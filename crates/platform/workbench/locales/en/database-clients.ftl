# Zen Workbench: a connection's command-line client in the Terminal and types generated from tables.

database-client-tab = { $tool }: { $name }
database-client-not-found = The command-line client of { $name } ({ $tool }) is not installed or not on PATH.
database-client-unsupported = { $name } has no command-line client the IDE can start.
database-client-tunnel-closed = { $name } goes through an SSH tunnel: connect it first, then open the client.
database-client-tunnel-unsupported = The command-line client of { $name } cannot go through its SSH tunnel.
database-client-sentinel = { $name } is a Redis Sentinel deployment; open the client on its primary instead.
database-client-no-file = { $name } names no database file.
database-error-no-columns = { $name } has no columns to make a type from.
database-error-columns-not-read = The columns of { $name } were not read yet: connect, then try again.
database-client-secret-file = The password file of the client could not be written: { $error }
database-client-agent-guarded = Agents do not open the command-line client of { $name }: it is read-only or asks before changes, and a client runs whatever is typed into it without asking.
