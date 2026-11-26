# Docker plug-in changelog

Every release of the Docker plug-in (`plugins/dockerfile`), newest first; the format is in
[../README.md](../README.md).

## 0.5.0 - 2026-11-15

### Changed

- The Docker tool window has an icon of its own (three stacked boxes, drawn in the theme's colours)
  and opens on the right the first time; you can move it, and it stays where you put it.

## 0.4.0 - 2026-11-13

### Added

- A Docker tool window: containers, images, volumes and networks, listed through the `docker`
  command (so with the person's Docker context and logins); start, stop, restart, a container's
  log and a shell in it, pull, and delete and prune, which ask the person every time. The plug-in
  only reads the listings; the IDE runs `docker` (ADR-0289).

## 0.3.0 - 2026-11-13

### Added

- Completion of Dockerfile instructions, their flags (`COPY --from=`, `RUN --mount=`, `FROM
  --platform=`…) and the parser directives, and problems: an unknown instruction or flag, an
  instruction other than `ARG` before the first `FROM`, an instruction without arguments, an
  `ONBUILD` that holds `FROM`, `MAINTAINER` or `ONBUILD`, and the deprecated `MAINTAINER`. All in
  the plug-in, without the network and without any program installed.
- Completion of Compose file keys and of the values of keys that take one of a few, and problems:
  a key the Compose Specification does not have where it stands, a value a key does not take, and
  the obsolete `version`. The key tree is the plug-in's own, from the public specification.
- Run icons beside `services:` and each Compose service: `docker compose -f <file> up [service]`,
  run by the IDE (the plug-in declares the one program it runs, `docker`).

### Removed

- The Dockerfile language server (a Node.js program from npm) is no longer declared: a server
  installed before is not started, and its settings break nothing.

## 0.2.0 - 2026-10-16

### Changed

- Renamed from "Dockerfile" to "Docker". It also highlights Docker Compose files (`compose.yaml`,
  `docker-compose.yml` and their variants), told from Dockerfiles by their first line.

## 0.1.0 - 2026-10-16

### Added

- The first release: highlighting for `Dockerfile`, `Containerfile` and their variants:
  instructions, flags, strings, variables, heredocs, comments and parser directives.
