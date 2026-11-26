# Zen Workbench: the terminal sessions. English is the source language (ADR-0024).

# The title of the first terminal tab without a name of its own (the shell's title, when short, comes first).
terminal-tab-local = Local
# The title of the second, third... terminal tab without a name of its own.
terminal-tab-local-numbered = Local ({ $number })
terminal-session-start-failed = Cannot start the terminal: { $error }
terminal-session-unsupported = The terminal is not available in this version
# Shown in a terminal tab whose session runs in a terminal host of an older IDE that this IDE cannot read.
terminal-session-elsewhere = This terminal runs in an older terminal host (protocol { $protocol }) that this version of the IDE cannot show. Its programs keep running until they end, and this tab closes when that host stops.
terminal-session-none = There is no terminal session; open one with terminal.session.new
terminal-session-missing = There is no terminal session { $id }
terminal-tab-missing = There is no terminal tab { $tab }
terminal-session-no-folder = There is no folder { $path }
terminal-send-too-long = The text is longer than { $limit } bytes
terminal-link-empty = Name a link to open: a URL or a file path
terminal-link-no-file = No file or folder { $path } in the terminal's folder or the project
terminal-link-folder-in-editor = { $path } is a folder: it cannot open in the editor
terminal-link-action-not-offered = The action { $action } is not offered for a link of kind { $kind }
terminal-link-browser-user-only = Only a person opens a link in the browser: ⌘-click it in the Terminal, or copy it with the action copyLink
terminal-link-browser-failed = Cannot open the browser: { $error }
terminal-selection-none = Nothing is selected in the terminal
terminal-lines-out-of-range = The session has { $lines } lines: choose lines from 1 to { $lines }
terminal-paste-user-only = Only a person pastes the clipboard into the terminal: press ⌘V
# The line under the dimmed output of a terminal tab that came back after the computer restarted.
terminal-session-ended-restart = Session ended: the computer restarted
# The line under the dimmed output of a terminal tab that came back after its terminal host stopped.
terminal-session-ended-host = Session ended: the terminal host stopped
terminal-rerun-user-only = Only a person runs a command again after a restart: click Run Again in the terminal tab
terminal-rerun-none = This terminal has no command to run again
# Added to the "Session ended" line when the tab resumes an AI agent's conversation through its adapter.
terminal-agent-resuming = Resuming the conversation of { $agent }
terminal-agent-unknown = There is no agent adapter { $agent }; terminal.agent.list lists them
terminal-agent-bad-session = A conversation id has only letters, digits and - _ . : (at most 128)
terminal-agent-no-terminal = There is no terminal { $terminal } in this window
terminal-agent-no-host = Agents' conversations are resumed only where the terminal host runs (macOS and Linux)
terminal-agent-report-failed = Cannot keep the agent's conversation id: { $error }
