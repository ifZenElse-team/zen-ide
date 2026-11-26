ide-plugin-tool-refresh = Refresh
ide-index-tree = Tree
ide-index-graph = Graph
ide-index-filter = Filter by name, kind or path
ide-index-indexing = Reading the repository…
ide-index-empty = This project has no GitOps objects: no Flux, Argo CD, Kustomize or Helm files were found.
ide-index-truncated = The project passed the index limits; part of it is not shown.
ide-index-state-none = No cluster state yet: it is read with the Kubernetes window's context.
ide-index-state-missing = The cluster has no { $kind } { $name }.
ide-plugin-tool-empty = Nothing to show.
ide-plugin-tool-busy = Working…
ide-plugin-tool-confirm-rows = { $count ->
    [one] The chosen item is deleted. This cannot be undone.
   *[other] The { $count } chosen items are deleted. This cannot be undone.
}
ide-plugin-tool-confirm-section = This cannot be undone.
ide-plugin-tool-choice = { $choice }:
ide-plugin-tool-choice-none = Not chosen
ide-plugin-tool-choice-source = { $choice } from: { $files }
ide-plugin-tool-choice-missing = { $path } (missing)
ide-plugin-tool-and = { $first } and { $last }
ide-plugin-tool-moved = { $windows } now { $count ->
    [one] opens
   *[other] open
} on the { $side ->
    [left] left
    [right] right
   *[bottom] bottom
}; Restore Default Layout moves { $count ->
    [one] it
   *[other] them
} there.
ide-plugin-tool-input-range = A whole number from { $min } to { $max }.
ide-plugin-tool-input-forward = A local port from 1024 to 65535 and a remote port from 1 to 65535.
ide-plugin-tool-input-forward-hint = local : remote, on 127.0.0.1 only
