# Invalidate Caches and Repair IDE (zen_workbench::maintenance). English is the source language (ADR-0024).

maintenance-no-data-folder = The IDE has no data folder.
maintenance-agent-local-history = Only the person can clear Local History; agents cannot.
maintenance-repair-title = Repair IDE
maintenance-works = Everything Works Now
maintenance-continue = Continue
maintenance-more = More
maintenance-next = { $did } Next step: { $step }.
maintenance-last = { $did } If it is still broken, More offers Invalidate Caches and Restart.
maintenance-step-refresh = Refresh file system
maintenance-step-rescan = Rescan Project Indexes
maintenance-step-resync = Reopen Project and Re-sync
maintenance-step-drop = Drop Indexes and Reindex Current Project
maintenance-did-refresh = File system refreshed, open files reloaded: { $count }.
maintenance-did-rescan = The project's indexes are being read again in the background.
maintenance-did-resync = Language servers restarted: { $servers }. Run configurations read: { $configurations }. Git state refreshed.
maintenance-did-drop = The indexes were dropped and are being built again in the background.
maintenance-did-file = { $file }: the index read it again; the file was not changed on disk.
maintenance-did-file-reloaded = { $file }: reloaded from disk and read into the index again.
