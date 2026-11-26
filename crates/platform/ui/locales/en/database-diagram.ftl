# Zen UI: ER diagrams in the Database workspace.

ide-database-view-diagram = View Diagram
ide-database-diagram-canvas = ER diagram
ide-database-diagram-zoom-in = Zoom In
ide-database-diagram-zoom-out = Zoom Out
ide-database-diagram-actual-size = Actual Size (100%)
ide-database-diagram-fit = Fit to Window
ide-database-diagram-export = Export
ide-database-diagram-export-png = PNG Image…
ide-database-diagram-export-svg = SVG Image…
ide-database-diagram-export-mermaid = Mermaid Text…
ide-database-diagram-copy-mermaid = Copy as Mermaid
ide-database-diagram-copied = Mermaid text copied
ide-database-diagram-exporting = Exporting…
ide-database-diagram-exported = Exported to { $path }
ide-database-diagram-export-failed = Export failed: { $error }
ide-database-diagram-cancel = Cancel
ide-database-diagram-computing = Laying out the diagram…
ide-database-diagram-empty = No tables to show: the structure is not read yet, or the schema has no tables.
ide-database-diagram-counts = { $entities ->
    [one] 1 table
   *[other] { $entities } tables
} · { $relations ->
    [one] 1 relation
   *[other] { $relations } relations
}
ide-database-diagram-partial = some columns not read yet
ide-database-diagram-more = +{ $count } more
ide-database-diagram-open-hint = double-click to open
