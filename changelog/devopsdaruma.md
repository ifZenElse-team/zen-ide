# DevopsDaruma changelog

Every release of DevopsDaruma, the Zen IDE for DevOps (containers, infrastructure as code and
pipelines), newest first. Each section is the release notes users see as "What's new"; the format
is in [README.md](README.md).

## 26.11.10 - 2026-11-27

### Changed

- The Kotlin language server is listed as **Kotlin LSP** in **Settings → Languages & Frameworks →
  Language Servers**; the notice beside it still says whose software it is.
- The **About** window, the website and the application's copyright line say that Zen IDE is an
  independent project, not affiliated with any other IDE vendor, without a list of products.
- The link of an internal error report opens the project's issues on GitHub.

## 26.11.9 - 2026-11-17

### Added

- Programs the IDE starts get your login shell's environment: runs, language servers, plug-in
  tool windows and terminal tabs see the same `PATH`, `KUBECONFIG`, version managers and proxies as
  your terminal, even when the IDE was started from the Finder or Zen Toolbox. The shell is read
  once, in the background, when the IDE starts; a run configuration's own variables still win.
  Turn it off in Settings → Tools → Terminal → Environment.
- The Kubernetes window's header names the kubeconfig files its contexts come from, with a missing
  one marked.
- Plug-in tool windows have icons of their own on the tool window bar, in your theme's colours, and
  are listed in View | Tool Windows. The Docker and Kubernetes windows open on the right; you can
  move them, and they stay where you put them. If you had them somewhere else, the IDE says once
  how to move them.
- **Diff with Cluster** and **Apply to Cluster** for a Kubernetes manifest file, from its run icon,
  and for a folder of manifests, from the Project view's context menu. Both use the context and
  namespace chosen in the Kubernetes window. Diff opens each changed object in the diff viewer,
  with the cluster's object now on the left and the file's result on the right. Apply, and every
  other kubectl command that changes a cluster, runs only when you start it and confirm the whole
  command line; AI agents can never start one.
- **Cluster state in the GitOps window.** Each Flux object and Argo CD Application shows what the
  cluster says of it, in the context chosen in the Kubernetes window: a coloured dot, and in a
  details pane its Ready, Synced and Health state, the revision applied and the last error, with
  **Show Events**.
  - It is read with `kubectl get` every 30 seconds while the window is open.
  - Without a cluster, the graph stays and says why.
  - Nothing in the cluster is changed.
- **GitOps problems in the editor.** A missing path or source, a HelmRelease without its repository,
  a duplicate name or a `dependsOn` cycle is underlined where it is and listed in Problems.
- **Go to Declaration and Find Usages in GitOps repositories.** ⌘B leads from a `sourceRef`,
  `dependsOn`, `path`, chart, project or kustomization entry to what it names; ⌥F7 lists what names
  an object.
- **GitOps repositories.** DevopsDaruma comes with the GitOps plug-in, which recognises Flux, Argo
  CD, Kustomize and Helm in a repository and builds its graph of sources, applications, releases,
  charts and folders, with the broken paths and references in it. AI agents read it with
  `plugins.index.get`; the GitOps tool window and the problems in the editor follow.
- **The GitOps tool window**, on the right: a Flux or Argo CD repository's sources, Kustomizations,
  Applications, HelmReleases, charts and folders as a tree and as a graph. A click highlights an
  object's connections, a double-click opens its file at its line, and problems are marked.
- Plug-in tool window actions can ask for values you type: the Kubernetes window's **Port
  Forward…** (pods and services, on 127.0.0.1 only) and **Scale…** (deployments). Values are
  checked before anything runs, and an AI agent never types them: it asks, and you enter them.
- **Helm charts and Kustomize overlays** in the GitOps plug-in:
  - ⌘B on `.Values.image.tag` in a template leads to the key in the chart's values files, and ⌥F7 on a key lists the templates that use it;
  - keys complete after `.Values.`, and a key no values file has is a warning;
  - a missing patch file of a `kustomization.yaml` is an error;
  - **Preview with helm template** and **Preview with kustomize build** open the rendered YAML read-only. They run locally and download nothing.
- **Settings → Languages & Frameworks → Go → Go Modules** sets the project's Go environment
  (`GOPROXY`, `GOPRIVATE`, `GOFLAGS`…): shared with the project in `.zen/go.json`, or only on this
  computer, where your values win. It reaches every `go` command the IDE runs for the project
  (runs, tests, debugging, the go.mod checks, `go vet`, installing gopls and Delve) and gopls. The
  shared file applies only in a trusted project and never holds a password.

### Changed

- The New Project wizard's **Environment** is saved with the new Go project in `.zen/go.json`
  instead of being used for `go mod init` only.

### Fixed

- The Kubernetes window lists your kubeconfig's contexts and the cluster's resources: before, it
  showed no context at all ("There is nothing to choose for Context"). The run icon on a shell
  script's first line appears too.
- A background task that starts or ends no longer moves the status bar's widgets (line and column,
  encoding, indentation) under the pointer: its progress shows to their left.
- A security fix in database connections over SSH: the IDE took the tunnel as open as soon as
  anything accepted a connection on its local port, so another program listening there could have
  received the database's credentials. It now waits for ssh itself to report the forward on that
  port, and fails the connection if ssh exits or never reports it.

## 26.11.8 - 2026-11-14

### Added

- **The first DevopsDaruma.** The Zen IDE for DevOps joins the family under the shared version: the
  IDE window, the editor, Git, the terminal, run configurations and MCP for AI agents, as in every
  Zen IDE, with Kozō and a daruma on the splash and welcome screens.
- **Ready for DevOps work**: Dockerfiles and Compose files, YAML with the schemas the IDE ships,
  GitLab CI, shell scripts, `.env` and ignore files, the Terraform plug-in (terraform-ls, and
  `init`, `validate`, `plan` and `apply` from the run icon, `apply` always asked to you) and the
  Kubernetes tool window over your clusters, with the Database workspace on. Go and Rust are a
  switch away in Settings → Plugins → Built-in.
