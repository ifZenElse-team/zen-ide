# SqlScroll changelog

Every release of SqlScroll, the Zen IDE for SQL and databases, newest first. Each section is the
release notes users see as "What's new"; the format is in [README.md](README.md).

## 26.11.11 - 2026-11-28

### Fixed

- The buttons of the **Trust and Open Project** dialog, and the outlined buttons of the other
  dialogs, keep their size and place under the pointer: only their colour changes.

## 26.11.10 - 2026-11-27

### Changed

- The Kotlin language server is listed as **Kotlin LSP** in **Settings → Languages & Frameworks →
  Language Servers**; the notice beside it still says whose software it is.
- The **About** window, the website and the application's copyright line say that Zen IDE is an
  independent project, not affiliated with any other IDE vendor, without a list of products.
- The link of an internal error report opens the project's issues on GitHub.

### Fixed

- An Oracle connection whose SQLcl finds no Java says "Java is missing" every time: on a busy
  machine that has another Java installed it could say only "SQLcl ended".

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
- Plug-in tool window actions can ask for values you type: the Kubernetes window's **Port
  Forward…** (pods and services, on 127.0.0.1 only) and **Scale…** (deployments). Values are
  checked before anything runs, and an AI agent never types them: it asks, and you enter them.
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

- **The first SqlScroll.** The Zen IDE for SQL and databases joins the family under the shared
  version: the IDE window, the editor, Git, the terminal, run configurations and MCP for AI agents,
  as in every Zen IDE, with Kozō and a scroll on the splash and welcome screens.
- **The Database workspace is there from the first start**: connections, the Database Navigator and
  SQL consoles, with the SQL plug-in; nothing connects until you ask it to. Go and Rust are a switch
  away in Settings → Plugins → Built-in.
