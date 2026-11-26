# Zen UI: the headless IDE for AI agents (ADR-0084). English is the source language (ADR-0024).

ide-headless-window-only = `{ $tool }` is not available in a headless IDE: keys need a drawn window. Use run_action with the key's action, or dispatch_command with a command (find_action and list_commands list them).
ide-headless-running = { $project } is already open in a headless { $product } (process { $pid }) at { $url }
ide-headless-no-server = cannot start the MCP server on port { $port }
ide-headless-usage = { $error }; usage: { $program } --headless [--mcp-port <port>] [--exit-with-parent] [--accept-open] [--no-restore] [<project>...]
ide-headless-bad-port = `{ $value }` is not a port
ide-headless-no-port = --mcp-port needs a port
ide-headless-unknown = unknown option `{ $option }`
ide-headless-servers-found = { $project }: language servers found: { $servers }
ide-headless-servers-none-found = { $project }: no language server it needs is installed
ide-headless-server-missing = { $project }: { $server } is missing; lsp.server.install with the id { $server } installs it (it runs { $command })
ide-headless-server-missing-manual = { $project }: { $server } is missing: { $hint }
ide-headless-safe-mode = { $project } is in safe mode: its language servers and programs do not start. An agent cannot change that, and no command line does: the person opens the project once in a window of the IDE on this data folder and trusts it there.
