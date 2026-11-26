# ADR-0042: zen-cli output and exit codes

- **Status:** Proposed
- **Date:** 2026-10-17
- **Refines:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

ADR-0007 names `zen-cli` among the clients that dispatch commands by id, and ADR-0033 makes every
capability reachable by AI agents. Until now the only way in from outside was the MCP server inside a
running IDE (`zen-automation`), which needs a window, a port and an enabled setting. Agents, scripts
and tests also need a plain program: list the commands, run one, read the result and tell success
from failure by the exit code alone. Once scripts depend on it, its output and exit codes are a public
interface, so they are written down before the first script relies on them.

## Decision

1. **The binary** is `zen-cli` (`apps/zen-cli`). It is headless: it depends on no UI crate and opens
   no window. It is a development and automation tool and is not released as a product for now.
2. **The registry** holds the commands of `zen-workbench` and `zen-toolbox`. Each invocation is a
   fresh session: no project is open at the start, and nothing persists after the program exits.
   The CLI does not initialise Toolbox state, so `toolbox.*` commands fail cleanly, with no side
   effects and no network access (AGENTS.md rule 10).
3. **`zen-cli commands [--json]`** lists the commands sorted by id. As text, each line is the id and
   the title in aligned columns. With `--json`, the output is `{"commands": [...]}`, where each entry
   is `zen_core::CommandInfo` as it serialises: `id`, `title`, `description`, `paramsSchema` and
   `aliases`.
4. **`zen-cli run <id> [--params <json> | --params -]`** dispatches one command. The parameters
   default to `{}`; `-` reads them from standard input. The result is printed to standard output as
   pretty-printed JSON, or `null` when the command returns nothing.
5. **Errors** are one line on standard error, `zen-cli: <message>`, localised through Fluent in
   English and Russian (ADR-0024). Command-line errors are one line too, ending with
   `see zen-cli --help`. The one exception is `zen-cli` with no subcommand, which prints the help to
   standard error and exits with 2.
6. **The language** of help and errors comes from `ZEN_LANGUAGE`: `en` or `ru`; `system`, an empty
   or unset variable, or any other value follows the system language, falling back to English.
   Command titles and descriptions are not translated.
7. **Exit codes:**

   | Code | Meaning |
   |---|---|
   | 0 | Success |
   | 2 | Usage error: bad arguments, invalid JSON, unknown command, invalid parameters |
   | 1 | The command is unavailable, failed, or returned invalid output; standard input could not be read; the output could not be written (then nothing is printed to standard error) |

8. Every subcommand has `--help` (`-h`), which prints the help to standard output and exits with 0.
   `zen-cli --version` (`-V`) prints `zen-cli <version>` the same way.

## Consequences

- Agents and scripts get a stable machine interface: JSON on standard output, one error line on
  standard error, and an exit code that separates the caller's mistakes (2) from the command's
  failures (1).
- The JSON of `commands --json` follows `CommandInfo`; renaming one of its fields is a breaking
  change to this interface and needs a new ADR.
- Commands that need an open project can only be chained within one invocation once the CLI can
  open one; until then a project opened by `file.project.open` is gone when the program exits.
- Out of scope for now, each a later decision: attaching to a running IDE, a `--project` option that
  opens a project before the command runs, and running several commands in one batch.

## Alternatives considered

- **Only the MCP server:** needs a running IDE with automation enabled; too heavy for scripts and
  tests, and the stdio bridge (`gogarden mcp`) still starts the whole IDE.
- **One exit code for every failure:** simpler, but a script could not tell a typo in its own call
  from a command that failed.
- **Plain text results:** easier to read, but every caller would have to parse them; JSON matches
  what the MCP server returns.
