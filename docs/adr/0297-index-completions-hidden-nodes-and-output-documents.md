# ADR-0297: Index completions, nodes outside the graph and run output as a document

- **Status:** Accepted (the owner, 2026-11-16)
- **Date:** 2026-11-16
- **Extends:**
  - [ADR-0286](0286-plugin-run-targets.md): plug-in run targets;
  - [ADR-0293](0293-gitops-project-index.md): the project index;
  - [ADR-0296](0296-a-manifest-against-the-cluster.md): `targetFiles` and `"output": "diff"`.

  All three stay as accepted; this record adds to them.

## Context

The GitOps plug-in learns Helm charts and Kustomize overlays: where each `.Values.x.y` of a template
comes from, and what the chart or overlay renders to. Three things are missing from the platform:
- **Names that are only targets.** A values key is a node of the index only so that references
  (Go to Declaration, Find Usages) can point at it. A chart has hundreds of keys, and the index
  tool window would show each of them in its tree and graph.
- **Completion across files.** Completion asks only the plug-in that handles a file. Templates are
  `.yaml` files, handled by the YAML plug-in, while the keys come from the GitOps plug-in's index of
  other files.
- **A rendered result to read.** `helm template` and `kustomize build` print YAML. In the Run console
  it has no highlighting, no structure and is cut at the console's buffer.

## Decision

1. **Nodes outside the graph.** An index node may say `"inGraph": false` (default `true`). Index
   tool windows (tree, graph and `plugins.toolWindow.rows`) leave such nodes out. Their references,
   Go to Declaration, Find Usages and `plugins.index.get` treat them as any node.
2. **Index completions.**
   - **What:** the index answer may carry
     `"completions": [{"files": [path], "after": str, "items": [{"label", "detail"?}]}]`.
   - **When:** completion in one of `files` (project-relative) looks at the 256 bytes before the
     caret for the last `after`. When everything between it and the caret is letters, digits, `_`,
     `.` or `-`, the items whose label starts with that text are offered, replacing it. In such a
     place they are the whole list: the language server and the file's plug-in are not asked.
   - **The file's state:** the completion is used while the file is being edited, before the index
     is built again; the names are those of the last index.
   - **Limits:** the host keeps at most 1000 completions and 2000 items in each; a label or detail is
     one line of at most 200 characters, `after` at most 64; a file that was not in the index's input
     is dropped. One completion offers at most 2000 items. The matching is a lookup in the last
     index, without a call into the plug-in, so it costs no time while typing.
3. **`"output": "document"` for run targets.**
   - **The target:** `"output": "document", "documentType": "yaml"` (ASCII letters and digits, at
     most 16, `txt` otherwise). It runs as any run, in the Run window, under the same trust, agent
     and person-only rules (ADR-0286, ADR-0291).
   - **The result:** when the run exits with 0, its standard output opens as a read-only editor tab
     named "Output of <command line>" with the document type's extension. The command line shows the
     target's file relative to the project. Any other exit code leaves the result in the console.
   - **Where it lives:** the text is kept in the IDE's cache of generated documents, outside the
     project, as the effective POM is. It cannot be edited or saved, so it never lands on a project
     file. The same target on the same file reuses its tab, which shows the new output.
   - **Size:** at most 16 MiB of standard output, from the start, in whole lines, are kept. A longer
     output is cut there, and the console says so. The console's own buffer does not cut it.
4. **Helm and Kustomize previews** (the GitOps plug-in) use the third point:
   - **Targets:** `helm template` on a chart, and `kustomize build` on an overlay, local and read-only.
   - **Nothing beyond the local files is read:**
     - `helm template` never gets `--dependency-update`, so missing chart dependencies stay an error
       of `helm`;
     - an overlay that names a remote resource (a URL or a Git address) gets no target at all;
     - kustomize never gets `--enable-alpha-plugins`, `--enable-exec` or `--enable-helm`, and helm
       never gets `--post-renderer`, so neither runs another program.
   - **Agents:** these targets only read; they follow the plug-in's agent setting and are not
     person-only.

## Consequences

- A chart's values keys can be targets of navigation without filling the GitOps window.
- Any plug-in with an index can complete names that live in other files: values keys, resource
  names, keys of a properties file.
- Any target that renders text (a chart, an overlay, a generated config) can open the result as a
  read-only document with highlighting.

## Alternatives

- **Asking every plug-in with an index for completion:** a call into WebAssembly on every typed
  character, for every plug-in. Rejected for the lookup in the built index.
- **Letting completion merge the plug-in's items with the language server's:** the YAML server has
  nothing useful after `.Values.`. Rejected; the place decides.
- **A temporary file in the project for the preview:** it could be committed or saved over.
  Rejected.
