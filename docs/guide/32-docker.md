# Docker

The **Docker** plug-in reads Dockerfiles and Docker Compose files. It is built into every Zen IDE,
works without the network and needs no program installed: everything it does is in the plug-in.

## Dockerfiles

Files named `Dockerfile`, `Containerfile`, `*.Dockerfile`, `Dockerfile.*` and `Containerfile.*` are
Dockerfiles; a Compose file of a Dockerfile's name is told apart by its content.

- **Highlighting** of instructions, flags, strings, variables, heredocs, comments and the parser
  directives at the top (`# syntax=`, `# escape=`, `# check=`).
- **Completion** (⌃Space, or as you type): the instructions at the start of a line and after
  `ONBUILD`, the flags of the instruction (`COPY --from=`, `RUN --mount=`, `FROM --platform=`,
  `HEALTHCHECK --interval=`…), `CMD` and `NONE` after `HEALTHCHECK`, and the parser directives in
  the comments above the first instruction.
- **Problems**, underlined and in the Problems tool window: an unknown instruction, a flag the
  instruction does not take, an instruction other than `ARG` before the first `FROM`, an
  instruction without arguments, an `ONBUILD` that holds `FROM`, `MAINTAINER` or `ONBUILD`, and
  `MAINTAINER`, which is deprecated (a warning). Lines continued with the escape character
  (`\`, or `` ` `` after `# escape=` `` ` ``) and heredoc bodies are read as Docker reads them.

Earlier versions used the Dockerfile language server, a Node.js program installed from npm. The
plug-in no longer uses it: a server installed before is not started, its settings are left
alone, and its folder under the IDE's data folder (`language-servers/docker-langserver`) can be
deleted.

## Docker Compose files

Files named `compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml` and their
`compose.*.yaml` and `docker-compose.*.yml` variants are Compose files.

- **Highlighting** of keys, strings, numbers, constants, anchors and `${VAR}` interpolation.
- **Completion** of the keys of the Compose Specification where the caret is: the top-level
  sections, a service's keys (`image`, `build`, `ports`, `depends_on`, `deploy`…), and the keys
  inside them (a long-syntax port, a mount, `build`, `healthcheck`, `deploy`, `develop`…), without
  the keys already there; and the values of keys that take one of a few (`restart`,
  `depends_on`'s `condition`, `deploy`'s `mode`, a port's `protocol`, a mount's `type`…).
- **Problems**: a key the Compose Specification does not have where it stands (a warning, as the
  specification grows), a value a key does not take (an error), and the obsolete top-level
  `version` (a warning). Keys starting with `x-` are extensions and are never reported; values
  with `${VAR}` are not checked.

The key tree is the plug-in's own, written from the public Compose Specification; nothing is
downloaded. YAML syntax errors come from the YAML language server, which downloads no schemas.

**Running services.** A run icon beside `services:` runs `docker compose -f <file> up` (all
services), and one beside each service runs `docker compose -f <file> up <service>`, in the file's
folder, with the `docker` program found on `PATH` or in the usual folders. The run shows in the Run
tool window and Services; Stop stops `docker compose`. Add `--build`, `-d` or other arguments in
Run/Debug Configurations (**Program arguments**). A project you have not trusted runs none of them,
and AI agents may not start them unless you allow it ([Plug-ins that run programs](10-plugins.md#settings--plugins)).

## Docker tool window

The **Docker** button on the tool window bar (on the right, three stacked boxes), or **View | Tool
Windows | Docker**, opens a window that lists, in four sections, what the `docker` program on this
computer knows: **Containers** (name, image, status, ports, with a dot for running, paused or
stopped), **Images** (name and tag, size, age), **Volumes** and **Networks**. It uses the `docker`
command, so it sees the same daemon as your terminal: your current Docker context (`docker context
use`), a remote daemon and your registry logins all apply.

- The open section is listed again every 5 seconds, and at once with **Refresh**. If `docker`
  fails (the daemon is not running), the window shows its message and tries again after 30
  seconds; **Refresh** tries at once.
- Click a row to choose it; Cmd-click or Shift-click chooses more. The toolbar and the row's
  context menu hold the section's actions.
- Containers: **Show Log** (double-click) follows its log in the Run tool window, **Open Shell**
  opens `sh` in it in the Terminal, and **Start**, **Stop** and **Restart** do what they say.
  Images: **Pull** fetches the image again.
- **Delete**, and the section's **Delete Stopped Containers**, **Delete Unused Images**, **Delete
  Unused Volumes** and **Delete Unused Networks** (`docker ... prune`), ask you first every time:
  they cannot be undone.
- A project you have not trusted lists and runs nothing here.
- AI agents may read the lists. Starting, stopping, a log or a shell follow **Agents may run its
  targets** in Settings → Plugins → Docker ([Plug-ins that run programs](10-plugins.md#settings--plugins));
  a shell lets an agent run anything inside the container. Deleting always asks you in a
  notification, whatever that setting says.

Agents read the same completion and problems with `editor.completion.*` and `problems.file.list`
([AI agents](11-ai-agents.md)).
