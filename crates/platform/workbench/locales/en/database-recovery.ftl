# Zen Workbench: what the Database workspace brings back after a crash.

database-recovery-title = Recovered after a crash
database-recovery-tab-title = Recovered
database-recovery-found = { $count ->
    [one] The IDE stopped without closing. One unsaved item was kept: review it before anything is run.
   *[other] The IDE stopped without closing. { $count } unsaved items were kept: review them before anything is run.
}
database-recovery-review = Review
database-recovery-console-restored = The unsaved text of { $name } was brought back as unsaved changes. Undo returns to the text saved.
database-recovery-sql-header = Unsaved changes of { $table } on { $connection }, recovered after a crash. Nothing was run: review before running.
database-error-unknown-recovered = There is no recovered item { $id }.
database-error-recovered-rolled-back = A transaction cannot be brought back: the server rolled it back when the connection ended. Its statements are in the query history.
database-recovery-confirm-discard = { $count -> [one] One recovered item holds unsaved text or edits; *[other] { $count } recovered items hold unsaved text or edits; } discarding loses them for good.
