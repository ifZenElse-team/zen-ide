# Release manifest and release server

> **Last updated:** 2026-10-19 · **Decisions:** [ADR-0008](adr/0008-toolbox-and-updates.md),
> [ADR-0020](adr/0020-release-server.md) · **Code:** `crates/platform/update` (`zen-update`),
> `apps/zen-releases`

## 1. The manifest (schema 1)

`manifest.json` lists every product and its releases. It is served next to `manifest.json.sig`, the
ed25519 signature of its exact bytes, in lowercase hex. Clients verify the signature with a trusted
public key **before** parsing, and verify each artifact's size (when given) and SHA-256 before
unpacking it.
Unknown fields are rejected; a different `schema` number means "update the client first".

```json
{
  "schema": 1,
  "products": [
    {
      "id": "rustronin",
      "name": "RustRonin",
      "releases": [
        {
          "version": "0.2.0",
          "channel": "stable",
          "date": "2026-10-16",
          "notes": "Markdown shown as What's new.",
          "artifacts": [
            {
              "platform": "macos-aarch64",
              "url": "artifacts/rustronin/0.2.0/rustronin-0.2.0-macos-aarch64.tar.gz",
              "sha256": "…64 hex digits…",
              "size": 12345678
            }
          ]
        }
      ]
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `products[].id` | Product id: the product name in lowercase (`rustronin`, `toolbox`) |
| `releases[].version` | A semantic version; the newest per channel and platform wins |
| `releases[].channel` | `stable` or `nightly` |
| `artifacts[].platform` | `<os>-<arch>` as Rust names them: `macos-aarch64`, `linux-x86_64`, `windows-x86_64` |
| `artifacts[].url` | Relative to the manifest's URL; always under `artifacts/` |
| `artifacts[].size` | The file's size in bytes, written by `zen-releases publish` from the uploaded file. Zen Toolbox shows it as the download size ("Update to 0.38.7 · 41 MB") and checks the download against it. Optional for readers: a manifest without it (hand-made, or from an older publisher) still parses, the size reads as 0 (unknown), Toolbox asks the server for it once per run with a `HEAD` request (`Content-Length`) and checks the download by its SHA-256 only |
| `releases[].notes` | Release notes in English Markdown, shown as "What's new" in Zen Toolbox and on the website |

Notes use a small Markdown subset, which Toolbox renders (`zen_toolbox::markdown`) and the website
renders the same way: headings, `-`/`*` and numbered lists, paragraphs, fenced code, inline
`` `code` ``, `**bold**`, `*italic*`, and links (shown as their text). Anything else stays plain text.

## 2. Artifacts and installation

An artifact is a `.tar.gz` whose `bin/<id>` (`bin/<id>.exe` on Windows) is the program Toolbox
launches; on macOS an IDE's artifact holds its application bundle, `<Name>.app`, instead, which
Toolbox copies into the Applications folder ([ADR-0025](adr/0025-macos-applications.md)); Zen Toolbox's own artifact contains `bin/zen-toolbox` and, on macOS, `Zen Toolbox.app`: a Toolbox running
from the bundle replaces the whole bundle, so that its icon and `Info.plist` update too, and every
replaced bundle is registered again with Launch Services to refresh cached icons ([ADR-0022](adr/0022-toolbox-updates-itself.md)).

Toolbox launches a tool with `toolbox.tool.launch` (`{ "id": "gogarden", "path": "/work/app" }`;
`path` is optional) and passes the project folder as the IDE's command-line argument
(`gogarden [folder...]`): `bin/<id> <folder>`, or on macOS `open -n -a <Name>.app --args <folder>`.
Launch Services hands `--args` only to a copy it starts, so a launch with a folder starts a new copy
of the IDE; without a folder, `open <Name>.app` brings a running copy to the front
(`zen_update::install::Launch::command_line`). `toolbox.project.open` launches the same way, in
the IDE that opened the folder last ([recent-projects.md](recent-projects.md)).

Toolbox installs into its tools directory (`zen-update::install`):

```text
<tools>/<id>/<version>/   an unpacked release
<tools>/<id>/current      the active version
```

Archives are unpacked into a hidden directory and renamed into place when complete; the pointer
files are replaced atomically. Only the active version is kept
([ADR-0029](adr/0029-no-rollback-for-now.md)). Going back is installing an older release from the
signed manifest again: `toolbox.tool.versions` lists a tool's releases on the selected channel for
this platform, and `toolbox.tool.installVersion` (`{ "id": "gogarden", "version": "0.2.0" }`)
installs one the way an update does; an AI agent is refused it
([ADR-0265](adr/0265-versions-updates-notifications-and-waiting-launchers.md)).

## 3. The server

```sh
# Once: a publisher key pair. The secret key never goes to the server.
zen-releases keygen --out publisher.key          # prints the public key

# Serve, trusting that public key. Uploads need the token.
ZEN_RELEASES_TOKEN=<token> zen-releases serve --data ./releases --public-key <public key hex>

# Publish an artifact into the signed manifest.
ZEN_RELEASES_TOKEN=<token> zen-releases publish --key publisher.key \
    --product rustronin --name RustRonin --version 0.2.0 --channel stable \
    rustronin-0.2.0-macos-aarch64.tar.gz
```

| Method and path | Purpose |
|---|---|
| `GET /releases/manifest.json`, `GET /releases/manifest.json.sig` | The signed manifest |
| `GET /releases/artifacts/<product>/<version>/<file>` | An artifact |
| `GET /api/v1/health` | Liveness |
| `PUT /api/v1/artifacts/<product>/<version>/<file>` | Upload an artifact (bearer token) |
| `GET /api/v1/artifacts/<product>/<version>/<file>` | The stored artifact's `{ size, sha256 }`, 404 when absent (bearer token): `zen-releases` uploads only what the server lacks |
| `PUT /api/v1/manifest` | Publish a manifest; signature in `X-Zen-Signature` (bearer token) |
| `GET /releases/changelog.json` | Every changelog, for the website (not signed; informational) |
| `PUT /api/v1/changelog` | Publish the changelogs (bearer token) |
| `PUT /api/v1/check/manifest`, `PUT /api/v1/check/plugins` | Check a signed manifest or catalog as its upload would, storing nothing (bearer token): `zen-releases` asks first, so that a server too old for a new field of the format refuses before any artifact or module is uploaded; `zen-releases check-credentials` checks the key and the token this way |
| `PUT /api/v1/stage/<stage>/manifest`, `…/plugins` | Stage a signed manifest or catalog, checked as its upload is, without publishing it; `X-Zen-Base` is the SHA-256 of the live document it was made from, or `none` (bearer token, ADR-0171) |
| `PUT /api/v1/stage/<stage>/changelog`, `PUT /api/v1/stage/<stage>/site/<path>` | Stage the changelogs, or one file of the website (bearer token) |
| `GET /api/v1/stage/<stage>` | What a stage holds and whether it is released (bearer token) |
| `POST /api/v1/stage/<stage>/release` | Make the stage live at once: its manifest, catalog, changelogs and website (`site/current` in the data folder); refused while the live manifest or catalog is not the one the stage was made from; a released stage answers the same again (bearer token) |
| `GET /releases/stage/<stage>/<manifest.json\|plugins.json\|changelog.json>[.sig]`, `GET /releases/staged-site/<stage>/<path>` | A staged document or website, to check before it is released |

The server accepts a manifest only if the signature verifies with its trusted public key and every
listed artifact is stored with the stated size and hash. It binds to loopback by default; in An artifact that the live manifest
or catalog (or the stage's earlier document) already lists the same is checked by its size alone:
it was hashed when it was first published, and a manifest lists every past release again (221
artifacts, 4.5 GB by 26.10.3), which made each upload re-read them all. At start-up the server
checks that it can write its data folders (`artifacts`, `stages`, `site`) and stops with what to do
when it cannot.
production Caddy stands in front of it at `ide.ifzenelse.space`: HTTPS, plus plain HTTP without a
redirect on the domain and on the server IP for the clients released before HTTPS (ADR-0040; see
`deploy/releases/README.md`).

The data directory is the whole state: `manifest.json`, `manifest.json.sig`, `artifacts/`.

## 4. Local development

```sh
cargo xtask dev-server                  # key, server on 127.0.0.1:8787, test releases; prints the Toolbox command
cargo xtask dev-publish rustronin 0.2.0 # publish a newer test release while it runs
```

Everything lives in `target/dev-releases/`. Test products are scripts that show a "test build"
dialog; the Toolbox artifact is a real build with a raised version, to try self-update.
