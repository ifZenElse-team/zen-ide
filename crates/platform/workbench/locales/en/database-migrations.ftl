# Zen Workbench: migration tools of the project in the Database workspace.

database-migration-title = Migrations
database-migration-read-failed = The table of applied migrations could not be read.
database-migration-ssh = { $name } goes through SSH, which the IDE opens only for itself: migration tools cannot reach it. Run them from a terminal with your own tunnel.
database-migration-driver = { $name }: migration tools run against PostgreSQL, MySQL, MariaDB and SQLite only, not { $driver }.
database-migration-no-down = { $tool } cannot revert a migration; write a new migration instead.
database-migration-needs-state = { $tool } reverts to a named migration, so the applied migrations are read first: connect and open the Migrations tab.
database-migration-unknown-tool = { $tool } is not a migration tool the IDE knows.
database-migration-unknown-direction = { $direction } is not a direction: use up or down.
database-migration-no-folder = No { $tool } migrations are in { $folder } any more.
database-migration-password-unread = The password of { $name } is not read yet: run the migration once from the Migrations tab of the Database workspace, which reads it from the Keychain.
database-migration-not-found = { $program } was not found on PATH; install { $tool } to run its migrations.
database-migration-unknown-set = { $id } is not a migration folder of this project any more; see the folders found on the Migrations tab.
database-migration-unknown-set-agent = { $id } is not a migration folder of this project; list them with `db.migration.list`.
database-migration-name-up = { $tool } up · { $name }
database-migration-name-down = { $tool } down · { $name }
database-migration-confirm-up = Apply { $count } pending { $tool } migrations to { $name }?
database-migration-confirm-down = Revert the last { $tool } migration ({ $migration }) on { $name }?
database-migration-risk-type = Connections of type { $kind } ask before anything changes them.
database-migration-risk-down = Reverting a migration usually drops tables or columns, and their data with them.
database-migration-read-only = { $name } is read-only: migrations change it, so nothing was run.
database-migration-agent-read-only = On { $name } ({ $kind }) agents may only read: a person runs migrations there, from the Migrations tab.
database-migration-nothing-applied = No { $tool } migration is applied, so there is nothing to revert.

workbench-dbmigration-kind = Database Migration
workbench-dbmigration-field-tool = Tool
workbench-dbmigration-field-folder = Migrations folder
workbench-dbmigration-field-connection = Connection
workbench-dbmigration-field-direction = Direction
workbench-dbmigration-field-target = Revert to (Django)
database-migration-password-in-url = { $tool } takes a password only in a URL or its environment, where other programs can read it, so the IDE gives it none. Run it in the terminal with your own configuration, or use a connection without a password.
database-migration-bad-name = { $name } is not an app name; it would be read as an option.
database-migration-not-approved = Start migrations from the Migrations tab (Run Migrations), which checks the connection and asks first; a saved configuration or Rerun does not.
database-migration-password-unsupported = Handing a password to migration tools is not supported on this system yet.
database-migration-not-connected = Connect { $name } first: what will run is read from the migrations it has applied.
