# Kubernetes

The **Kubernetes** plug-in adds a tool window over your clusters. It works in every Zen IDE:
DevopsDaruma comes with it, and the other IDEs install it from **Settings → Plugins →
Marketplace**. It uses the `kubectl` you have installed (found on `PATH` and in the usual folders,
such as Homebrew's), with your kubeconfig, so it sees the clusters your terminal sees and signs in
as `kubectl` does. It downloads nothing, and it never changes your kubeconfig.

## The Kubernetes tool window

The **Kubernetes** button on the tool window bar (on the right, six dots around a centre), or
**View | Tool Windows | Kubernetes**, opens the window.

**The header names the cluster.** Below the title, **Context** and **Namespace** say where
everything in the window is listed and done. They start from your kubeconfig's current context and
that context's namespace (or `default`), and you change them in the window's drop-downs. That
choice belongs to the window: it is not written into your kubeconfig (no `kubectl config
use-context`), so other terminals and tools keep their own. The IDE remembers it for the
project. Every command the window runs passes it explicitly (`kubectl --context <context>
--namespace <namespace> …`), so a `use-context` run in a terminal never moves the window to
another cluster.

Below them, the header names the kubeconfig files the contexts come from, as `kubectl` in your
terminal reads them: the files of `KUBECONFIG` as your login shell sets it, or `~/.kube/config`.
A missing file is marked. `kubectl`, and the authentication plug-ins it runs (`gke-gcloud-auth-plugin`,
`kubelogin`, `aws`), get the same `KUBECONFIG` and `PATH` as in your terminal
([Environment](07-terminal.md#environment)).

If the context you chose is no longer in your kubeconfig, the window says so in red and lists and
does nothing until you choose another. If you may not list a cluster's namespaces, the window
keeps working in the context's own namespace.

**Sections:**

- **Pods:** name, ready containers, status (`Running`, `CrashLoopBackOff`, `Terminating`…),
  restarts, node and creation time, with a dot: green when running and ready, yellow while
  starting, red when failing.
- **Deployments:** name, ready replicas, up to date, available, creation time.
- **Services:** name, type, cluster IP, ports.
- **ConfigMaps:** name and number of keys.
- **Secrets:** by name only. The window lists them with `kubectl get secrets -o name`, so their
  values are never read.
- **Events:** the newest first, warnings marked.
- **Namespaces** and **Contexts:** what the header chooses from, with each context's cluster,
  user and namespace.

The open section is listed again every 5 seconds, and at once with **Refresh**. If `kubectl`
fails (the cluster cannot be reached), the window shows its message and tries again after 30
seconds. Click a row to choose it; Cmd-click or Shift-click chooses more.

**Actions** (on the toolbar and in a row's context menu):

- Pods: **Show Log** (double-click) follows the pod's log (all containers, the last 200 lines
  first) in the Run tool window, **Open Shell** opens `sh` in the pod in the Terminal, **Describe**
  shows `kubectl describe` in the Run tool window.
  **Port Forward…** asks for a local and a remote port and runs `kubectl port-forward` in the Run
  tool window; Stop ends it.
- Services: **Describe** and **Port Forward…**, with the service's first port filled in as the
  remote one.
- Deployments: **Describe**, **Restart Rollout** (`kubectl rollout restart`), **Scale…**, which asks
  for the number of replicas (0 to 1000).
- **Delete** in every section but Events, Namespaces and Contexts.

**Every change asks you first, naming the cluster:** "Restart api in the context prod,
namespace shop?" or "Delete web-1 in the context prod, namespace shop? This cannot be undone." Read
the context before you confirm: a wrong cluster is the usual mistake.

**A forward listens on 127.0.0.1 only**, on the local port you type (1024 to 65535): other computers
on your network cannot reach it. A port already in use makes the run fail with kubectl's message.

**Scaling names the count too:** "Scale api in the context prod, namespace shop, to 3 replicas?"

## Manifests and the cluster

A manifest file (a YAML or JSON file with `apiVersion` and `kind` at the top) has a run icon on
its first `apiVersion` line with two targets:

- **Diff with Cluster** runs `kubectl diff` with the context and namespace chosen in the Kubernetes
  window, so it never follows a context changed elsewhere. Each object that differs opens in the
  diff viewer, with **Live now (Context …, Namespace …)** on the left, what is in the cluster, and
  **After applying <file>** on the right, what the file would make of it. With no difference, the
  Run window says so.
- **Apply to Cluster** runs `kubectl apply` in the same context and namespace. You confirm it
  every time, in a dialog that shows the whole command line: the context, the namespace and the
  file.

A folder of manifests has the same two, for every manifest below it (`-R`), in the Project view's
context menu (**Diff Folder with Cluster**, **Apply Folder to Cluster**). With no context chosen
in the window yet, they ask you to choose one first.

Every kubectl command that changes a cluster (`apply`, `create`, `replace`, `patch`, `delete`,
`edit`, `scale`, `rollout`, `label`, `annotate`, `set`, `drain`, `cordon`, `uncordon`, `taint`)
runs from the IDE only when you start it and confirm it, every time. AI agents can never start
one, whatever their access; they may run Diff as **Agents may run its targets** allows, and read
its output. Nothing runs in a project you have not trusted or in safe mode.

## Safety and AI agents

- A project you have not trusted lists and runs nothing here.
- AI agents may read every section and the header's choices (`plugins.toolWindow.rows`). Only you
  change the context or namespace.
- A log, a describe or a shell follow **Agents may run its targets** in **Settings → Plugins →
  Kubernetes** ([Plug-ins that run programs](10-plugins.md#settings--plugins)); a shell lets an
  agent run anything inside the pod.
- Restarting and deleting always ask you in a notification with the same question, naming the
  context, whatever that setting says.
- An agent never types a port or a count. When it asks to forward a port or to scale, a notification
  names the action, the rows and the context, and **Enter Values…** opens the window's dialog for
  you to type them; nothing runs until you do.
