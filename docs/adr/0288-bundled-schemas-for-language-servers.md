# ADR-0288: Schemas ship inside plug-ins; servers never download them by themselves

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Builds on:** [ADR-0031](0031-plugins.md) (WebAssembly plug-ins),
  [ADR-0050](0050-products-are-plugin-bundles.md) (every capability a plug-in),
  [ADR-0055](0055-language-servers.md) (language servers),
  [ADR-0282](0282-third-party-notices-in-every-release-artefact.md) (third-party notices)

## Context

The YAML language server checks and completes a file against a JSON Schema. On its defaults it
downloads a public catalog of schemas and every schema a file names, which golden rule 10 forbids;
an earlier change turned the catalog off, so YAML files lost their schema checks. A schema can
also be named in a file (`# yaml-language-server: $schema=<url>`) or reached by a `$ref`, and the server fetches an
`http(s)` one itself.

People still want GitHub workflow, GitLab CI and Kubernetes files completed and checked,
offline, and their own schemas for their own files.

The basis is the YAML server's documented settings and protocol (its README): `yaml.schemas` maps
a schema URI to file patterns; a schema whose URI scheme the server does not know is asked from
the client with the request `custom/schema/content`; after the client's notification
`yaml/registerContentRequest`, the server asks the client for `http(s)` schemas too, with
`vscode/content`, instead of fetching them. The server's own source was looked at only to confirm
this protocol behaviour; nothing of its code or text is taken (rule 2).

## Decision

1. **A plug-in may ship schemas.** It declares them in `plugin.json`:

   ```json
   "schemas": [{ "id": "gitlab-ci", "name": "GitLab CI/CD", "server": "yaml-language-server",
                 "entry": "ci.json", "files": [".gitlab-ci.yml", "*.gitlab-ci.yml"] }]
   ```

   and gives their text through a new optional export of ABI v1, `zen_resource`: input the
   resource's path (UTF-8, `/`, no `..`), output its bytes (empty when it has none). Schemas live in
   the module, so a plug-in from the catalog carries them as a bundled one does.
2. **The platform serves them to the server.** For a server named in a declaration, the platform
   answers the server's settings (`yaml.schemas` for the YAML server) with the declared schemas as
   `zen-schema://<plugin>/<entry>` and their file patterns, and answers the server's request for a
   schema's content (`custom/schema/content` for the YAML server) from the plug-in's `zen_resource`.
   A `$ref` relative to a schema resolves inside the same plug-in (Kubernetes' `all.json` and
   `_definitions.json`). The file names a schema in a comment with the same URI
   (`$schema=zen-schema://yaml/kubernetes/all.json`) or with a project file's path.
3. **No download by itself.** The platform asks the YAML server to send it every `http(s)` schema
   request (`yaml/registerContentRequest`), and refuses them: the server shows the file's problem
   "schema downloads are off". A person may turn downloads on (**Settings → Languages &
   Frameworks → Schemas → Allow downloading schemas a file names**, off by default, the person's
   alone); then the IDE downloads the named schema itself and keeps it for the session:
   - only `http://` and `https://` addresses; never a `file://` outside the project;
   - never in safe mode (a project nobody trusted);
   - a notification names the full address **before** the download starts.
4. **A person's own mappings.** The same Settings page lists the shipped schemas with their
   patterns and the person's own rows: a schema (a shipped one, or a JSON Schema file of the
   project or the computer) and the file patterns it applies to. They are commands
   (`schemas.catalog.list`, `schemas.mappings.set`), stored in the IDE's settings, and reach the server as
   (1) does; a person's row wins over a shipped pattern for the same file.
5. **Notices.** A shipped schema is third-party data: its licence and copyright go into the
   module's `zen-licences` section (ADR-0282) from `plugins/<id>/schemas/NOTICES` and into
   `ATTRIBUTION.md`. Only schemas under permissive licences are shipped; each `NOTICES` names the
   source, the version and the licence.
6. **What ships first:** the GitLab CI plug-in ships the GitLab CI schema (MIT); the YAML plug-in the GitHub workflow schema
   (from SchemaStore, Apache-2.0) and a Kubernetes schema (the standalone strict schemas generated
   from Kubernetes' OpenAPI, Apache-2.0), the latter for `*.k8s.yaml`, `k8s/**/*.yaml` and
   `kubernetes/**/*.yaml` by default and any file a person maps.

Compose files are not given a schema: the Docker plug-in checks and completes them itself, without
the YAML server (Node.js), and a schema would report their keys twice.

## Consequences

- YAML files get completion and checks again, offline, from the plug-in that knows the format.
- A schema update is a plug-in release; nothing changes under a person's feet.
- Plug-in modules grow by their schemas (the Kubernetes ones by about 1.3 MB).
- The same mechanism serves the JSON server and others later, which download `$schema`s today
  (to be checked per server).

## Alternatives

- **Files next to the module** (a `schemas/` folder in the plug-in's folder): a plug-in from the
  catalog is one module; we would need a second distribution format.
- **Blocking downloads with a dead proxy setting:** hides the reason from the person and still
  lets other requests through; the content request is the server's own way to hand them over.
- **Our own YAML schema checker:** duplicates a mature server; out of scope.
