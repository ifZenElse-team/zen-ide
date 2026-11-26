# Zen toolchain: the errors of the toolchain commands. English is the source language (ADR-0024).
toolchain-error-none = This IDE has no language toolchain.
toolchain-error-unknown-sdk = { $root } is not a Go SDK found on this computer or added.
toolchain-error-no-go = { $root } is not a Go SDK: it has no bin/go.
toolchain-error-no-version = { $root } is not a Go SDK: its go command does not tell its version.
toolchain-error-not-added = { $root } is not a Go SDK added by hand.
toolchain-error-no-gopls = { $path } does not exist.
toolchain-error-not-gopls = { $path } is not gopls: it does not answer "gopls version".
toolchain-error-no-sdk = No Go SDK is installed.
toolchain-error-no-sdk-install = No Go SDK is installed; install Go from https://go.dev/dl/ first.
toolchain-error-gopls-missing = gopls is not installed.
toolchain-error-installed-not-found = go install finished, but gopls was not found in GOPATH/bin.
toolchain-error-save = Cannot save { $file }: { $error }
toolchain-error-run = Cannot run { $program }: { $error }
toolchain-error-no-dlv = { $path } does not exist.
toolchain-error-not-dlv = { $path } is not Delve: it does not answer "dlv version".
toolchain-error-dlv-installed-not-found = go install finished, but dlv was not found in GOPATH/bin.
toolchain-error-agent = An agent may not add, remove or choose a Go SDK, gopls or Delve: it chooses the program that runs. The person can do it in Settings → Languages & Frameworks → Go.
