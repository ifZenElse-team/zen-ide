# ADR-0293: GitOps repositories: a plug-in's project index, its problems, navigation and graph

- **Status:** Accepted (the owner, 2026-11-15)
- **Date:** 2026-11-15
- **Builds on:** [ADR-0050](0050-products-are-plugin-bundles.md) (every capability a plug-in),
  [ADR-0031](0031-plugins.md) (WebAssembly plug-ins), [ADR-0289](0289-plugin-tool-windows.md) and
  [ADR-0290](0290-tool-window-choices-and-confirmations.md) (plug-in tool windows),
  ADR-0292 (a plug-in window's icon and side),
  [ADR-0033](0033-ai-access-to-every-api.md) (every action a command), [ADR-0285](0285-devopsdaruma-app.md)

## Context

A person opens a GitOps repository, one that Flux or Argo CD reconciles into a cluster, and wants
to see what it holds: which applications, where each comes from, and how they connect. They want
to catch a broken reference before it reaches the cluster, and to jump from a reference to what it
names. All of this is in the repository's YAML: Flux and Argo CD objects, `kustomization.yaml`
files and Helm charts. Nothing needs a cluster, a program or the network.

What the platform offers plug-ins today does not reach this:

- every text export (`zen_highlight`, `zen_problems`, `zen_outline`...) sees one file, while these
  references cross files (a `Kustomization` in `clusters/prod/apps.yaml` names a `GitRepository`
  in `clusters/prod/flux-system/gotk-sync.yaml` and a folder `./apps/prod`);
- the tool windows of ADR-0289 list rows that a declared program prints. Here nothing runs, and the
  window must show a graph as well as rows.

## Decision

### 1. Where it lives

- **The plug-in `gitops`** (`plugins/gitops`) knows Flux, Argo CD, Kustomize and Helm. It
  recognises, parses and checks, and it works in every Zen IDE. DevopsDaruma bundles it; the other
  IDEs offer it from the Marketplace when they open a repository it recognises.
- **The platform gets generic pieces only**, each usable by any plug-in:
  - a project index export;
  - its problems in the editor;
  - Go to Declaration through it;
  - a graph section in plug-in tool windows.

  No Flux or Argo CD word is in the platform.

### 2. A project index (new optional export of ABI v1, `zen_index`)

- **Which files.** A plug-in declares the files it reads in `plugin.json`:
  `"index": {"files": ["*.yaml", "*.yml"]}`.
- **What the platform sends.** It walks the project the way Find in Files does: ignored and
  excluded folders are skipped, and so are `.git`, `node_modules` and the IDE's own folders. It
  hands the plug-in UTF-8 JSON:
  `{"files": [{"path": str, "text": str}], "paths": [str], "remotes": [str]}`.
  - `files` are the matching files, paths relative to the project with `/`.
  - `paths` are every file and folder of the project, so the plug-in can tell whether a referenced
    path exists without reading it.
  - `remotes` are the fetch URLs of the project's Git remotes, read by the platform's Git layer
    (`zen_vcs::Repository::remote_urls`), so the plug-in can tell whether a URL names this
    repository without reading `.git` (the owner, 2026-11-15).
  - Limits: 5000 matching files, each at most 1 MiB, 64 MiB in all, and 100 000 paths. Past a
    limit, the index is marked `truncated` and says so.
- **What the plug-in answers.** UTF-8 JSON:

  ```json
  {"kinds": ["flux"],
   "nodes": [{"id": "flux:Kustomization/flux-system/apps", "kind": "Kustomization", "group": "flux",
              "name": "apps", "namespace": "flux-system", "path": "clusters/prod/apps.yaml",
              "start": 120, "end": 410, "detail": "./apps/prod"}],
   "edges": [{"from": "flux:Kustomization/flux-system/apps", "to": "flux:GitRepository/flux-system/flux-system",
              "kind": "source"}],
   "references": [{"path": "clusters/prod/apps.yaml", "start": 260, "end": 271,
                   "target": "flux:GitRepository/flux-system/flux-system"}],
   "problems": [{"path": "clusters/prod/apps.yaml", "start": 300, "end": 312, "severity": "error",
                 "message": "No folder ./apps/prod in this repository"}]}
  ```

  - `nodes` are what the graph shows; a folder or a chart is a node too, and its `id` is
    `path:<folder>` or `chart:<folder>`.
  - `edges` connect node ids, with a `kind` the graph names: `source`, `path`, `chart`,
    `dependsOn`, `resource`, `project`, `child`.
  - `references` are spans that name a node, for Go to Declaration.
  - `problems` are spans to show in the editor.
  - Byte offsets are into the file's text as sent.
- **What the platform does with the answer.** It keeps the answer per project and asks again 500 ms
  after a matching file is saved, created, deleted or moved on disk. The call runs off the UI
  thread, with the export's usual fuel and memory limits, and a result larger than 32 MiB of JSON
  is dropped. It drops nodes, edges, references and problems that point outside the files sent or
  past a file's end. It keeps at most 20 000 nodes, 50 000 edges, 50 000 references and 5000
  problems.
- **Never in the way.** The walk and the call run on a background thread, never the UI thread
  and never one a language server needs. They stop as soon as the project closes or a newer
  change starts another index. The first index of a large repository fills in when it is done:
  until then the window says it is indexing, and the editor works as before.
- **Read-only by construction.** The plug-in only reads the text it is handed. It has no imports
  (ADR-0031), so it cannot open a file, run a program or reach the network. The platform runs
  nothing for an index.

### 3. Problems in the editor and Problems view

An index's problems for a file are shown in the editor and the Problems tool window, and returned
by `problems.file.list`, beside the language server's and the plug-in's own `zen_problems`. Their
source is the plug-in's name.

### 4. Go to Declaration through the index

Go to Declaration (⌘B, `navigate.declaration.goTo`) works like this:

- On a span of an index `reference`, it goes to the target node's file and offset.
- A `path:` target opens the folder in the Project view.
- A target that is not in the index (a missing source) says so, with the problem's message.
- It is tried before the language server, because the YAML server knows nothing of these names.
- Find Usages of a node (`navigate.usages.find`) lists the index references to it.

### 5. A graph in a plug-in tool window

A tool window section may say `"rows": "index"` instead of a `list` command (ADR-0289): its rows are
the index's nodes, and it shows them two ways.

- **Tree.** The nodes are grouped as the plug-in says. A node with an edge of kind `source`,
  `path`, `chart` or `child` from a parent sits under that parent, and nodes no edge reaches are at
  the top.
- **Graph.** The nodes are boxes in layers, sources on the left and the things they feed to the
  right, with `dependsOn` edges drawn dashed. A layered layout is computed by the platform; it pans
  and zooms, and fits at once.
- **In both views:**
  - a click selects a node and its connections are highlighted;
  - a double-click (or ⏎) opens its file at its line;
  - a node with problems is marked, and a filter field narrows the nodes by name, kind or path.

A section with `"rows": "index"` runs no program and has no `actions` that run one. The window is
declared like any other, with ADR-0292's `"icon"` and `"side": "right"`; this section type is drawn
by a render path of its own beside ADR-0289's rows.

### 6. The GitOps model (the `gitops` plug-in)

- **Recognition.** A repository is a GitOps repository when its YAML holds objects of these API
  groups, in any document of a multi-document file:
  - **Flux:**
    - `source.toolkit.fluxcd.io`: `GitRepository`, `OCIRepository`, `HelmRepository`, `Bucket`,
      `HelmChart`;
    - `kustomize.toolkit.fluxcd.io`: `Kustomization`;
    - `helm.toolkit.fluxcd.io`: `HelmRelease`;
    - the bootstrap folder `flux-system`, with `gotk-components.yaml` and `gotk-sync.yaml`.
  - **Argo CD** (`argoproj.io`): `Application`, `ApplicationSet`, `AppProject`.
  - **Kustomize:** every `kustomization.yaml`, `kustomization.yml` and `Kustomization` file.
  - **Helm:** every folder with a `Chart.yaml`.

  `kinds` names what was found (`flux`, `argocd`, `kustomize`, `helm`). An index with no Flux or
  Argo CD object and no Kustomize or Helm file is empty, and the window says the project is not a
  GitOps repository.
- **Nodes.** One node for:
  - each object above;
  - each folder a `path`, `resources` or generator names;
  - each local chart.

  A node's `name` is `metadata.name`, and its `namespace` is `metadata.namespace`, else the
  namespace the folder's `kustomization.yaml` sets.
- **Edges.**
  - **Flux `Kustomization`:**
    - `spec.sourceRef` → source (`source`; the namespace defaults to the referring object's);
    - `spec.path` → folder (`path`; relative to the repository root);
    - `spec.dependsOn[]` → Kustomization (`dependsOn`).
  - **`HelmRelease`:**
    - `spec.chart.spec.sourceRef` → HelmRepository, GitRepository or Bucket (`source`);
    - `spec.chart.spec.chart` → local chart folder, when the source is a GitRepository of this
      repository (`chart`);
    - `spec.chartRef` → OCIRepository or HelmChart (`source`);
    - `spec.dependsOn[]` (`dependsOn`).
  - **Argo CD `Application`:**
    - `spec.source.path` and `spec.sources[].path` → folder (`path`);
    - `spec.source.chart` → chart (`chart`);
    - `spec.project` → AppProject (`project`).
  - **`ApplicationSet`:** `spec.template.spec.source.path` and `spec.generators[].git.directories[].path`
    (globs expanded against `paths`) → folders (`path`).
  - **App of apps:** an Application whose folder holds Applications gets `child` edges to them.
  - **A folder:** `resource` edges to the objects in it, and to what its `kustomization.yaml` lists
    in `resources`, `components` and `bases`.
  - A `repoURL` is shown as the node's detail. An Argo CD `path` is resolved in this repository.
    When the Application's `repoURL` is one of `remotes`, it is this repository. ssh and https
    forms of one URL are the same, and so are URLs with and without `.git`
    (`git@host:group/repo.git` = `https://host/group/repo`). In that case a missing folder is an
    error. Otherwise it is a warning, since it may be another repository's (the owner, 2026-11-15).
- **Problems.**
  - **Errors:**
    - a Flux `spec.path` or a `kustomization.yaml` entry that names no folder or file here;
    - a `sourceRef` or `dependsOn` to a kind and name no object in the repository has (a namespace
      that is not written matches any namespace, so a missing one is never reported falsely);
    - a HelmRelease whose chart's source is no `HelmRepository`, `GitRepository` or `Bucket` here;
    - two objects of one kind with the same name in the same namespace.
  - **Warnings:**
    - an Argo CD `path` not found here, when its `repoURL` is not one of this repository's remotes
      (an error when it is);
    - an Application whose `project` names no AppProject here (except `default`);
    - a `dependsOn` cycle.
- **What it never does.** It never evaluates Helm templates or Kustomize overlays, never resolves a
  `repoURL`, never reads `values` from outside the repository, and never decrypts anything (SOPS
  files are read as YAML, their values left alone).

### 7. Commands for agents

- `plugins.index.get { plugin, kinds?, path?, compact? }`: the index (nodes, edges and problems),
  narrowed by kind or by file.
- `plugins.index.refresh { plugin }`: index again now.
- Problems and navigation come through `problems.file.list`, `navigate.declaration.goTo` and
  `navigate.usages.find`, as for any language.
- The tool window's rows come through `plugins.toolWindow.rows`.

All of them are read-only. For an agent, nodes, references and problems in files hidden from
agents (the hidden-files patterns) are left out.

## Consequences

- A person sees a Flux or Argo CD repository as a graph and a tree, catches a broken path or a
  missing source as they type, and follows a reference with ⌘B, with no cluster and no network.
- The index, its problems, its navigation and the graph section are open to every plug-in: the
  same pieces can serve Terraform modules, Compose files that include others, or CI pipelines that
  include templates.
- Indexing reads every matching file of the project into the plug-in on each change (debounced),
  which costs a few hundred milliseconds on a large repository, off the UI thread. The limits keep
  a monorepo from stalling it, and `truncated` says when they cut.
- What the cluster does with these files (overlays rendered, Helm values merged, the live state)
  is out of scope: that needs `kustomize`, `helm` or `flux`/`argocd` and a cluster, and is a later
  decision.

## Alternatives considered

- **Parse in the platform:** puts Flux and Argo CD into the platform, against ADR-0050.
- **Per-file exports only, the platform joining the results:** the plug-in would have to export
  "what does this file declare and reference", and the platform would match names, kinds and
  namespaces with Flux's and Argo CD's rules (namespace defaults, `chartRef`, generators). That
  is domain knowledge in the platform again.
- **Run `flux build` or `argocd` locally to get the real picture:** needs the tools, often a
  cluster, and runs programs on the repository's content; out of this work's limits.
- **A UI ABI so the plug-in draws its own graph:** a much larger decision; the generic graph
  section serves every plug-in alike.
