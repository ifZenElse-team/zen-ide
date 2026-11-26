# Zen UI: an IDE's command line (ADR-0085). English is the source language (ADR-0024).

ide-cli-version = { $product } { $version }
ide-cli-about = { $product } { $version }, a Zen IDE
ide-cli-usage = Usage:
ide-cli-help-window-syntax = [<folder or file>...]
ide-cli-help-window = Opens the folders as projects, one window each, and a file (`file:line[:column]` at that place); without them, the projects that were open at the last quit. When the IDE already runs, it opens them there and this command ends at once.
ide-cli-help-wait-syntax = --wait <file>...
ide-cli-help-wait = Opens the files and ends when their tabs are closed, starting the IDE if it is not running: for `git config core.editor "{ $program } --wait"`.
ide-cli-help-headless-syntax = --headless [--mcp-port <port>] [--exit-with-parent] [--accept-open] [--no-restore] [<project>...]
ide-cli-help-headless = Runs the IDE without a window for AI agents; it prints its MCP server's address. With --accept-open it takes what a later start hands over, as a window does; with --no-restore the projects open without their last tabs and tool windows.
ide-cli-help-mcp-headless-syntax = mcp --headless [--no-restore] [<project>]
ide-cli-help-mcp-headless = MCP over standard input and output with a headless IDE for the project (the current folder by default): it starts with the agent's session and ends with the last one. For `claude mcp add -s project <name> -- <program> mcp --headless .`
ide-cli-help-mcp = MCP over standard input and output with the IDE's window, started when needed.
ide-cli-help-version = Prints the version.
ide-cli-help-help = Prints this help.
ide-cli-mcp-usage = { $error }; usage: { $program } mcp [--headless [--no-restore] [<project>]]
ide-cli-mcp-one-project = only one project can be given
ide-cli-mcp-project-needs-headless = a project is given only with --headless
ide-cli-no-data-folder = the application data folder is unknown
ide-cli-cannot-start = cannot start the headless IDE: { $error }
ide-cli-window-usage = { $error }; usage: { $program } [<folder or file>...]; { $program } --help lists every way to start
ide-cli-not-handed-over = no running { $product } of this data folder took the folders and files, and ZEN_HANDOFF_ONLY forbids a new window
ide-cli-wait-no-ide = the IDE did not start within a minute
