# GitOps plug-in changelog

Every release of the GitOps plug-in (`plugins/gitops`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-15

### Added

- The first release. It recognises Flux (`GitRepository`, `OCIRepository`, `HelmRepository`,
  `Bucket`, `HelmChart`, `Kustomization`, `HelmRelease`, the `flux-system` bootstrap), Argo CD
  (`Application`, `ApplicationSet`, `AppProject`), `kustomization.yaml` files and Helm charts in
  the project's YAML. DevopsDaruma comes with it, and every other IDE offers it from the
  Marketplace.
- It builds the project's GitOps graph: sources, Kustomizations and Applications, HelmReleases,
  charts and folders, joined by `sourceRef`, `path`, `chart`, `dependsOn`, projects, ApplicationSet
  directories, the app-of-apps pattern and the entries of each `kustomization.yaml`.
- It finds the broken pieces: a path or a kustomization entry that is not there, a source or a
  `dependsOn` that names nothing, a HelmRelease without its repository, the same name twice in a
  namespace, and `dependsOn` cycles. An Argo CD path is checked as an error when its `repoURL` is
  one of the project's Git remotes, and as a warning otherwise.
- The GitOps tool window, on the right: the objects as a tree and as a graph, a click highlighting
  an object's connections and a double-click opening its file at its line.
- Helm charts: Go to Declaration from `.Values.x.y` in a template to its key in the chart's values
  files and Find Usages back, completion of the keys after `.Values.`, and a warning for a key no
  values file has.
- A missing patch file of a `kustomization.yaml` is an error.
- **Preview with helm template** on a chart and **Preview with kustomize build** on a local
  overlay open the rendered YAML read-only, through the `helm` and `kustomize` you have installed.
- With the Kubernetes plug-in, the cluster state of each Flux object and Argo CD Application, read
  with `kubectl get` in the context chosen in the Kubernetes window: ready, synced, health, the
  revision applied and the last error. The GitOps window shows them as dots and in a details pane,
  with **Show Events**. It never changes a cluster.
- It downloads nothing: previews never update chart dependencies, and an overlay with remote
  resources offers none.
