# Deploying the release server

The release server ([ADR-0020](../../docs/adr/0020-release-server.md), format in
[docs/release-manifest.md](../../docs/release-manifest.md)) runs in Docker: the
`zen-releases` container stores releases in `/srv/zen-releases/data`, and Caddy in front of it serves
ports 80 and 443 for `ide.ifzenelse.space`.

## What Caddy serves

| Path | Served by |
|---|---|
| `/releases/*`, `/api/*` | the `zen-releases` container: the signed manifest, artifacts, health and publishing API |
| `/fonts/*` | `assets/fonts` (Inter, SIL Open Font License; `OFL-Inter.txt` is served next to it) |
| everything else | the website a release made live (`/srv/zen-releases/data/site/current`, a link to the stage it came with, [ADR-0171](../../docs/adr/0171-releases-from-a-protected-tag-through-gitlab.md)) when there is one; else any file in [`site/`](site/): the landing page `index.html`, `changelog.html` (renders `/releases/changelog.json`, published by every `cargo xtask release`), `img/kozo/` (the pictures of Kozō, written by `cargo xtask mascot` and committed), `docs/` (built from [docs/guide](../../docs/guide/README.md) by `cargo xtask site-docs`, run when the server is deployed), and `install.sh` (served as `text/plain`) |

The landing page has no build step, no external resources, no cookies and no trackers. Its primary
action is the Terminal install command `curl -fsSL https://ide.ifzenelse.space/install.sh | sh` (the
app is not notarised yet, so Gatekeeper blocks a downloaded `.dmg`). Its secondary download link
reads `/releases/manifest.json`, picks the newest `toolbox` release on the `stable` channel that has
a `macos-aarch64` artifact, and links to the installer uploaded next to the artifacts at
`/releases/artifacts/toolbox/<version>/Zen-Toolbox-<version>-macos-aarch64.dmg`; without such a
release it shows "Not available yet". Edit `site/index.html` to change it; the next release brings it.

## Limits and integrity

- The token is checked before a body is read. A manifest, catalog or changelog may be 32 MiB, a website
  file 64 MiB, and an artifact 4 GiB; an artifact is written to disk as it arrives.
- An artifact the live manifest or catalog lists cannot be replaced: an upload with other bytes gets
  409, the same bytes pass. An artifact that is only staged can be replaced, and releasing that stage
  then refuses it.
- Staging and releasing check every listed artifact by its SHA-256. The server keeps the hashes in
  memory while a file is unchanged, so the first staging after a restart reads every artifact once.
- A manifest or catalog is stored with its signature so that a crash between the two renames is
  finished at the next start.
- `/api/*` is refused over plain HTTP, all but `/api/v1/health`.
- `install.sh` verifies `manifest.json.sig` with the public key it carries and installs the archive
  that manifest lists, checked by its SHA-256, on a Mac as on Linux. The `.dmg` the website links to is
  not in the manifest: downloaded in a browser, it and its checksum file are covered by HTTPS only.

## Secrets

| What | Where | Leaves the publisher's machine? |
|---|---|---|
| Signing secret key | `~/.config/zen-ide/publisher.key` | Only into the protected, masked CI variable `ZEN_PUBLISHER_KEY` of the `release` environment (ADR-0171). Back it up offline; losing it means re-keying every Toolbox |
| Signing public key | `~/.config/zen-ide/publisher.pub`, compiled into Zen Toolbox, server `.env` | Yes, it is public |
| Upload token | `~/.config/zen-ide/releases.env`, server `/srv/zen-releases/.env` | To the server, and into the protected, masked CI variable `ZEN_RELEASES_TOKEN` |

Create the key and token once: `zen-releases keygen --out ~/.config/zen-ide/publisher.key` (writes
the secret key and prints the public key into `publisher.pub`), and a random token in
`releases.env` (`ZEN_RELEASES_SERVER=...` and `ZEN_RELEASES_TOKEN=...`).

## Deploy

The maintainers deploy the server; the steps and the host are theirs and are not part of this
repository. What a deployment does: build the image of this folder's `Dockerfile` from the
workspace, build the guide with `cargo xtask site-docs`, start `compose.yaml` with the server's
`.env` (the public key, the upload token and `ZEN_SITE_ADDRESS`), and check
`/api/v1/health`. Data survives redeployments. A tag release needs no deployment: it stages the
website through the server's token API and the release button makes it live (`site/current` in the
data folder, which Caddy serves read-only at `/srv/site-live`).

## Domain and HTTPS

`ZEN_SITE_ADDRESS` in the server's `.env` lists the site addresses, separated by `, `; the default
is `ide.ifzenelse.space, http://ide.ifzenelse.space`:

- `https://ide.ifzenelse.space` is the address people open in a browser; Caddy obtains and renews the
  certificate automatically.
- `http://ide.ifzenelse.space` serves the same content over plain HTTP, **without** a redirect to
  HTTPS. Listing an `http://` address explicitly is what turns Caddy's redirect off. The releases up
  to Toolbox 0.3 and GoGarden 0.20 use plain HTTP; later ones use HTTPS
  ([ADR-0040](../../docs/adr/0040-https-pure-rust-tls.md)). Keep it until no installed Toolbox or
  IDE uses it, then redirect.

Override it for a test, e.g. `ZEN_SITE_ADDRESS=:80` (plain HTTP on any host); such a test address
answers `/api/*` only for the health check, so publish to it from inside the Compose network.
