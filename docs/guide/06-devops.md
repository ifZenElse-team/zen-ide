# DevOps

DevopsDaruma is the Zen IDE for DevOps work: containers, infrastructure as code, pipelines and the
scripts around them, in the same IDE window as every Zen IDE, with Git, the terminal, Find in Files,
the HTTP Client and AI agents. It is a preview, released with the other IDEs since 26.11.8: install it
from Zen Toolbox ([Zen Toolbox](13-toolbox.md)).

Everything DevopsDaruma does is plug-ins, and every plug-in works in every Zen IDE. The ones DevOps
work needs most come with each of them ([Plug-ins](10-plugins.md)):

- **Docker**: Dockerfiles and Compose files;
- **YAML**, **JSON** and **TOML**;
- **GitLab CI**: `.gitlab-ci.yml`;
- **Shell scripts**;
- `.env` files and ignore files (`.gitignore`, `.dockerignore` and similar).

DevopsDaruma also comes with the **Terraform** plug-in: `.tf`, `.tfvars` and other HCL files, with
the Terraform Language Server ([Terraform](10-plugins.md#terraform)), and the **Kubernetes**
plug-in, a tool window over your clusters through your own `kubectl` ([Kubernetes](34-kubernetes.md));
the other IDEs install them from the Marketplace.

DevopsDaruma has nothing another Zen IDE cannot get; it is the one that comes ready for DevOps work.
The [Database workspace](28-database.md) is on, with the SQL plug-in; Go and Rust are off, and each
is one switch away in **Settings → Plugins → Built-in** (the change applies after a restart).

## Start DevopsDaruma

Once it is installed from Zen Toolbox, the command `devopsdaruma` starts it from a terminal, like
every Zen IDE:

```sh
devopsdaruma .                       # the current folder in DevopsDaruma
devopsdaruma Dockerfile:3            # the file at line 3
devopsdaruma --headless .            # DevopsDaruma without a window, for AI agents
devopsdaruma mcp --headless .        # the same as an MCP server on stdio
```

With **Settings → Tools → MCP Server → Enable MCP Server** on, AI agents reach it at
`http://127.0.0.1:63535/mcp` by default ([AI agents](11-ai-agents.md)).

## GitOps repositories

DevopsDaruma also comes with the **GitOps** plug-in. Open a repository that Flux or Argo CD
reconciles into a cluster, and the plug-in recognises it from its YAML:

- **Flux:** `GitRepository`, `OCIRepository`, `HelmRepository`, `Bucket`, `HelmChart`,
  `Kustomization` and `HelmRelease`, and the `flux-system` bootstrap;
- **Argo CD:** `Application`, `ApplicationSet` and `AppProject`;
- **Kustomize and Helm:** every `kustomization.yaml` and every Helm chart.

It builds the graph of how they connect: each Kustomization or Application to its source and
folder, each HelmRelease to its repository or chart, `dependsOn`, projects, the folders an
ApplicationSet generates from, and the applications an app-of-apps folder holds.

It finds what would break in the cluster:
- a path or a `kustomization.yaml` entry that is not in the repository;
- a source or a `dependsOn` that names nothing;
- a HelmRelease without its repository;
- a patch file of a `kustomization.yaml` (`patches`, `patchesStrategicMerge`, `patchesJson6902`)
  that is not there;
- the same name twice in one namespace;
- a `dependsOn` cycle.

An Argo CD path that is not in the repository is an error when the Application's `repoURL` is one
of this repository's Git remotes (ssh or https, with or without `.git`), and only a warning when it
names another repository. The plug-in reads the repository's files and needs no cluster; it runs
nothing unless you start a preview (below) and downloads nothing. It reads at most 5000 YAML files
of up to 1 MiB, in the background, and again half a second after one changes.

**The GitOps tool window** opens from the button on the right tool window bar, or with **View |
Tool Windows | GitOps**:
- **Tree** lists the entry points, the Kustomizations and Applications nothing else contains. Under
  each are its source, its folder and what the folder holds, its chart, its project and what it
  depends on, each labelled with how it connects.
- **Graph** draws the same objects as boxes in columns, sources on the left of what they feed;
  `dependsOn` lines are dashed. Drag to move it, and pinch or ⌘-scroll to zoom.
- **In both:**
  - a click highlights an object's connections;
  - a double-click (or ⏎) opens its file at its line, or shows a folder in the Project tool
    window;
  - an object with a problem is marked red or yellow;
  - the field above them filters by name, kind or path, and **Refresh** reads the repository
    again.

**In the editor:**
- Every problem it finds is shown where it is, underlined, and in the **Problems** tool window,
  like a compiler's. A file you are editing waits until it is saved: the problems follow the saved
  text.
- **Go to Declaration** (⌘B, or ⌘-click) on a reference leads to what it names:
  - from a `sourceRef` name to its `GitRepository`, `HelmRepository` or `OCIRepository`;
  - from a `dependsOn` name to its Kustomization or HelmRelease;
  - from a `path` to its folder, shown in the Project tool window;
  - from a local chart to its `Chart.yaml`;
  - from an Argo CD `project` to its AppProject;
  - from a `kustomization.yaml` entry to its file or folder.
- **Find Usages** (⌥F7) on an object's `metadata.name`, or on a reference, lists everything in the
  repository that names that object.

### Helm charts and Kustomize overlays

A folder with a `Chart.yaml` is a Helm chart: its `values.yaml` and `values-*.yaml` are its values,
and the files under its `templates/` (`.yaml` and `.tpl`) its templates.
- **⌘B on `.Values.image.tag`** in a template opens the key `tag` under `image` in `values.yaml`
  (or the first values file that has it). For a key whose value is empty (`podAnnotations: {}`), a
  deeper `.Values.podAnnotations.team` leads to that key.
- **⌥F7 on a key** in a values file lists the templates that use it, and the same key in the other
  values files.
- **Completion** after `.Values.` (⌃Space) offers the chart's keys, each with the
  values file it comes from.
- **A `.Values` key that no values file of the chart has** is a warning: it has to come from
  `--set`, `-f` or a parent chart. `.Values.global.…` is never reported.
- The keys are not in the GitOps tool window, so a large chart does not crowd it.

**Previews.** The run icon on a chart's `Chart.yaml`, and **Run Targets** in a chart folder's
context menu in the Project view, offer **Preview with helm template**. A `kustomization.yaml`
offers **Preview with kustomize build**. Each runs the `helm` or `kustomize` you have installed in
the Run window. When it succeeds, the rendered YAML opens in a read-only editor tab named after the
command, outside the project, so it can never be saved over a file of the repository. At most
16 MiB are shown, and the Run window says when the output was longer.
- **Local only.** `helm template` never updates the chart's dependencies: a missing dependency is
  `helm`'s error, not a download. A `kustomization.yaml` that names a remote resource (a URL or a
  Git address) offers no preview at all.
- **No other programs.** Neither kustomize's plug-in options nor helm's post-renderer is ever
  passed, so a preview runs no other program.
- **Trust and agents.** Like every run, a preview needs a trusted project. AI agents may start one
  only as **Settings | Plugins** allows for the GitOps plug-in, and read the YAML with
  `run.output.get`.

### State in the cluster

With the **Kubernetes** plug-in installed, the GitOps window also shows what the cluster says of
each Flux object (Kustomizations, HelmReleases, sources) and each Argo CD Application.
- **Which cluster:** the context chosen in the Kubernetes window's header. Choose another context
  there, and the GitOps state of the old one disappears at once, then is read again from the new
  one.
- **The dots:** each object gets a dot: green when it is ready (Flux) or synced and healthy (Argo
  CD), yellow while it is progressing or out of sync, red when it failed or is degraded, grey when
  it is suspended or unknown.
- **The details:** the pane under the tree and the graph says which context the state is from
  ("State from prod"). For the selected object it shows Ready, Synced, Health, the revision applied
  and the last error. **Show Events** lists the object's Kubernetes events in the Run window.
- **When it is read:** every 30 seconds while the window is open, and at once with **Refresh**.
  When the cluster cannot be reached, the graph stays as it is and the pane says why. The window
  then tries again every 2 minutes, or when you press **Refresh**.
- **Read-only:** the state is read with `kubectl get` from the kubectl you have installed, in
  trusted projects only. Nothing is changed in the cluster: reconciling or syncing is not part of
  this window. A cluster without Argo CD still shows the Flux state, and says that Argo CD's
  resources are missing.

AI agents read the graph with `plugins.index.get`
([AI agents](11-ai-agents.md)), and the problems and references through the usual
`problems.file.list`, `navigate.declaration.goTo` and `navigate.usages.find`. The other IDEs install
the plug-in from the Marketplace.
