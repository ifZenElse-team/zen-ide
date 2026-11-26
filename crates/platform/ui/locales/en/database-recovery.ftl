# Zen UI: the Recovered tab of the Database workspace.

ide-dbrecovery-note = Kept when the IDE stopped without closing. Nothing has been run against a database.
ide-dbrecovery-discard-all = Discard All
ide-dbrecovery-empty = Nothing to recover.
ide-dbrecovery-loading = Reading what was kept…
ide-dbrecovery-kind-console = Unsaved console
ide-dbrecovery-kind-edits = Unsaved data changes
ide-dbrecovery-kind-transaction = Rolled-back transaction
ide-dbrecovery-lines = { $count ->
    [one] { $count } line
   *[other] { $count } lines
}
ide-dbrecovery-rows = { $count ->
    [one] { $count } row
   *[other] { $count } rows
}
ide-dbrecovery-statements = { $count ->
    [one] { $count } statement, { $modifying } changing data
   *[other] { $count } statements, { $modifying } changing data
}
ide-dbrecovery-restore = Restore
ide-dbrecovery-open-sql = Open SQL in Console
ide-dbrecovery-discard = Discard
ide-dbrecovery-dismiss = Dismiss
ide-dbrecovery-console-explained = The console's text that was not saved. Restore opens the console with it as unsaved changes; Undo returns to the text saved.
ide-dbrecovery-edits-explained = The statements that would save the staged changes. Each update finds its row by the old key and checks that the values read are still there. Open SQL in Console to review and run them by hand.
ide-dbrecovery-transaction-explained = This transaction was open when the IDE stopped. The server rolled it back when the connection ended, so its changes are not in the database. Its last statements:
ide-dbrecovery-discard-question = Discard this recovered work? It cannot be restored afterwards.
ide-dbrecovery-discard-all-question = Discard all recovered work? It cannot be restored afterwards.
ide-dbrecovery-cancel = Cancel
