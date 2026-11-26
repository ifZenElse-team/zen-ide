# Kubernetes plug-in changelog

Every release of the Kubernetes plug-in (`plugins/kubernetes`), newest first; the format is in
[../README.md](../README.md).

## 0.3.3 - 2026-11-16

### Added

- The window's header names the kubeconfig files the contexts come from: the files of
  `KUBECONFIG` as your login shell sets it, or `~/.kube/config`, with a missing one marked. kubectl
  gets the same `KUBECONFIG` and `PATH` as in your terminal (Zen IDE 26.11.9 or later).

## 0.3.2 - 2026-11-16

### Added

- **Diff with Cluster** and **Apply to Cluster** for a manifest file (a YAML or JSON file with
  `apiVersion` and `kind`), from the run icon on its first `apiVersion` line, and for a folder of
  manifests from the Project view's context menu (`-R`). Both use the context and namespace chosen
  in the Kubernetes window. Diff opens each changed object in the diff viewer: the cluster's object
  now on the left, the result of the file on the right.
- Every kubectl command that changes a cluster (`apply`, `create`, `replace`, `patch`, `delete`,
  `edit`, `scale`, `rollout`, `label`, `annotate`, `set`, `drain`, `cordon`, `uncordon`, `taint`) runs
  only when you start it and confirm the whole command line, every time; AI agents can never start
  one. Needs Zen IDE 26.11.9 or later.

## 0.3.0 - 2026-11-16

### Added

- **Port Forward…** on pods and services: you type the local and the remote port (a service's first
  port is filled in), and `kubectl port-forward` runs in a Run tab, on 127.0.0.1 only; Stop ends it.
- **Scale…** on deployments: you type the number of replicas (0–1000), and the question names the
  context, namespace, deployments and the new count before anything changes. Agents never pass these
  values: you type them when an agent asks.

## 0.2.0 - 2026-11-15

### Changed

- The Kubernetes tool window has an icon of its own (six dots around a centre, drawn in the theme's
  colours) and opens on the right the first time; you can move it, and it stays where you put it.

## 0.1.1 - 2026-11-15

### Fixed

- The Kubernetes window lists your contexts and the cluster's resources: in 0.1.0 the
  module answered the IDE's ABI question with version 0, so the IDE never asked it to read
  kubectl's output, and the window showed no context ("There is nothing to choose for Context").

## 0.1.0 - 2026-11-13

### Added

- The first release: the Kubernetes tool window, through the `kubectl` you have installed.
  DevopsDaruma comes with it, and every other IDE offers it from the Marketplace. Needs Zen IDE
  26.11.7 or later.
- The context and namespace the window works in, chosen in its header and starting from your
  kubeconfig's. They are passed to every command (`--context`, `--namespace`) and never written
  into the kubeconfig.
- Sections for pods, deployments, services, config maps, secrets (by name only, never their
  values), events, namespaces and contexts.
- A pod's log and a shell in it, describe, restart a rollout, and delete. Restarting and deleting
  ask every time, naming the context and namespace.
