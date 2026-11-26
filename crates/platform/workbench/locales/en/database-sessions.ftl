# Zen Workbench: the sessions and locks of a connection's server in the Database workspace.

database-sessions-title = Sessions: { $name }
database-error-no-sessions = { $name } does not list its server's sessions; PostgreSQL, MySQL and MariaDB do.
database-error-session-id = { $id } is not a session id of this server.
database-error-session-read-only = { $name } is read-only: cancelling a query or ending a session changes the server, so nothing was sent.
database-error-agent-session = Agents may only read on { $name } ({ $kind }): cancelling queries and ending sessions are left to a person.
database-error-own-session = Session { $id } is one of this IDE's own sessions ({ $context }); confirm that it is yours to stop it.
database-session-confirm-cancel = Cancel the running statement of session { $id }{ $who } on { $name }? The session stays open.
database-session-confirm-terminate = End session { $id }{ $who } on { $name }?
database-session-risk-own = Session { $id } is one of this IDE's own sessions.
database-session-risk-terminate = Ending session { $id } rolls back its open transaction and closes its connection.
database-session-risk-query = It runs: { $query }
