# ADR-0074: Authors' pictures in the Git workspace, fetched only when turned on

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0057](0057-git-workspace.md), [ADR-0061](0061-http-client.md),
  [ADR-0033](0033-ai-access-to-every-api.md), AGENTS.md golden rule 10

## Context

The owner (2026-10-19) asked for avatars, a floating details panel and a collapsing graph:
commit nodes of the Git workspace's graph should show the author's picture instead of the initials.
A picture is not in the repository: it has to come from a service that
knows the author's e-mail address. Golden rule 10 says the IDE works offline and uses the network
only explicitly, visibly and with a way to turn it off. ADR-0057 left avatars from hosting services
out of the first stage.

Questions this decision answers: which servers are contacted, with what; when; where pictures are
kept; and what is shown otherwise.

## Decision

1. **Off by default; one setting.** Settings → Version Control → Git Workspace → **Show the
   authors' pictures from Gravatar, GitLab and GitHub** (`gitAvatars` of `settings.json`, the
   session setting `vcs.workspace.avatars`). While it is off nothing is contacted and no picture is
   shown, not even one kept from before: the nodes and the details panel show the initials, as
   they always did. The page names the servers next to the checkbox.
2. **What is contacted, in this order, for the authors of the rows in view only:**
   - the repository's **GitLab**, when a remote's host is a GitLab (`gitlab.com`, or a host named
     `gitlab.*`/`*.gitlab.*`): `GET https://<host>/api/v4/avatar?email=<address>&size=80`, GitLab's
     public avatar lookup, which needs no account; it answers with the picture's address, which is
     then fetched. **This sends the e-mail address itself** to the repository's own GitLab, which
     already has it in the repository it hosts;
   - **GitHub**, when a remote is on `github.com` and the address is a GitHub no-reply address
     (`<id>+<name>@users.noreply.github.com`): `https://avatars.githubusercontent.com/u/<id>`, or
     `https://github.com/<name>.png` for the older form without the id. Other addresses are not
     sent to GitHub: it has no public lookup by e-mail;
   - **Gravatar**: `https://gravatar.com/avatar/<hash>?s=80&d=404`, where `<hash>` is the SHA-256 of
     the trimmed, lower-case address (Gravatar's documented key). The address itself is not sent.
   No cookies, accounts or tokens; requests go through `zen-http` (the pure-Rust TLS stack of
   ADR-0040, 10-second timeout), at most four at once, in the background.
3. **Kept on disk, shared by every window.** A picture found is kept in the data folder as
   `caches/avatars/<hash>.img` (with its source in `<hash>.source`) and read from there afterwards
   without the network. An address every server answered without a picture is marked
   (`<hash>.none`) and asked about again after a week at the earliest. An address that could not be
   asked (offline, a server down) is not marked: it is tried again in a later session.
4. **Only what the platform decodes is kept**: a picture that `zen-image` cannot decode counts as
   none.
5. **Every state is a command** (ADR-0033): `vcs.avatar.get` answers for addresses with `off`,
   `pending`, `found` (the file and the source) or `none`, and can wait for the fetching; agents
   turn the setting on and off with `set_settings`.
6. **Tests never reach a real server**: the services' addresses are configurable inside the
   session (`Avatars::set_endpoints`) and the tests point them at `zen-http`'s local test server.

## Consequences

- With the setting on, the graph shows faces where the services have them and initials
  elsewhere; offline it falls back to the pictures already kept, then the initials.
- Turning it on discloses to Gravatar that someone looks at commits by these addresses (as hashes)
  and to the repository's GitLab which addresses are looked up; the settings page says so.
- Other hostings (Bitbucket, Gitea, self-hosted GitHub Enterprise) are not asked; they can be added
  under this decision by naming them here in a new ADR.
- The cache is never cleaned by itself; deleting `caches/avatars` in the data folder empties it.
