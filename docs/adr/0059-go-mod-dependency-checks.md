# ADR-0059: go.mod dependency checks over the network, and inspections with quick-fixes

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0040](0040-https-pure-rust-tls.md),
  [ADR-0055](0055-language-servers.md); AGENTS.md golden rule 10

## Context

The owner asked (2026-10-19) for dependency checks in go.mod: the editor colours versions teal,
underlines an outdated version ("Newer version is available: v1.2.6", with **Update dependencies…**
⌥⇧↵ and **More actions…** ⌥↵) and a module with known vulnerabilities (a list of advisories with
their scores, **Show details…**, **Update to unaffected version**), and counts them in the editor's
problems indicator. Both checks need the network, which the IDE does not use by default (golden
rule 10): the use must be explicit, visible and possible to turn off.

The editor so far knew only the problems language servers publish, without quick-fixes, and showed
them in a plain tooltip.

## Decision

1. **Inspections are a platform extension point.** `zen_project::inspect::Inspector` (L1) lets a
   language module or plug-in find problems in a file itself: `Finding`s with a severity (error,
   warning, or *weak warning*), a message, rows (e.g. one per vulnerability with a
   link and a badge), Markdown details and `InspectionFix`es (the first is the main one; a fix of
   kind `batch` opens a dialog of every fix with its batch key). An inspector never blocks: it looks
   things up in the background and changes its `generation` and wakes the window when it knows
   more. It declares its on/off settings (`DeclaredSetting`: key, settings page, group, title,
   description, default, and the servers it contacts) and applies fixes as background jobs.
   The workbench (`zen_workbench::inspections`) inspects the open files, merges the findings with
   the language servers' problems in `problems.file.list`, `problems.project.list` and F2, and adds
   `problems.fixes.list`, `problems.fix.apply`, `problems.fix.status`,
   `problems.inspections.refresh` and `settings.declared.list` (ADR-0033). The UI is shared by every
   IDE: the hover popup with the main fix (⌥⇧↵) and **More actions…** (⌥↵), **Show Context
   Actions** (⌥↵, `view.intentions.open` of ADR-0058, which lists the quick-fixes at the caret
   when there are some and the language server's context actions otherwise) and **Apply Main
   Quick-Fix** (⌥⇧↵, `view.problem.fix`), the batch dialog, the details dialog, weak warnings drawn quieter and counted
   apart, and the declared settings on their page (stored as `declaredSettings` in `settings.json`,
   changed by agents with `set_settings`).
2. **The go.mod checks live in the Go language module** (`zen_lang_go::deps`,
   `GoModInspector`), which is to become the Go plug-in bundled in GoGarden and installable in any
   Zen IDE (the owner, 2026-10-19): nothing Go-specific goes into the platform or the application.
3. **What is contacted.** Only public services that need no account, with read-only GET requests:
   - the **Go module proxy** of the project's toolchain (`GOPROXY` as `go env` reports it, by
     default `https://proxy.golang.org`): `<module>/@v/list`, and `<module>/@latest` when the list is
     empty, following the list's `,` and `|` fallbacks and stopping at `direct` or `off` (the IDE
     asks no version control system itself);
   - the **Go vulnerability database** (`https://vuln.go.dev`, or `GOVULNDB`): `index/db.json`,
     `index/modules.json.gz`, and `ID/<id>.json` for the entries that may affect a required version;
   - the **OSV database** (`https://api.osv.dev/v1/vulns/<GHSA id>`) for the CVSS vector and rating
     of an entry's GitHub advisory, because the Go database rates no severity. The score is computed
     with the public CVSS 3.1 formulas.
   A module that `GOPRIVATE`, `GONOPROXY` or `GONOSUMDB` names, or that a `replace` directive
   replaces, is never asked about; the vulnerability database is read whole, so module names are
   never sent to it. `GOFLAGS` is passed on to the `go` commands the fixes run.
4. **When.** When a go.mod is open in the editor (opened or changed). Answers are kept in memory
   and in `<data>/caches/go.mod/` (versions, the index and its modification time, the entries, the
   severities) and asked again only after an hour; the entries are fetched again only when the
   index says they changed. A failed request (offline) is retried after five minutes; nothing is
   shown and nothing fails without the network, and what an earlier run kept is shown offline.
5. **Turning it off.** Settings → Languages & Frameworks → Go → **Dependencies**: "Check for newer
   versions of dependencies" (`go.dependencies.checkUpdates`) and "Check dependencies for known
   vulnerabilities" (`go.dependencies.checkVulnerabilities`), both on by default, each listing the
   servers it contacts. Off, the check sends no request at all. Agents read them with
   `settings.declared.list` and change them with `set_settings` (`declaredSettings`).
6. **Fixes** run `go get module@version…` (and `go mod tidy` only when the user picks the fix that
   says so) with the project's toolchain in the module's folder, after saving go.mod, in the
   background; when they end, go.mod and go.sum are read again and the checks run again; a failure
   shows the end of `go`'s output as a notice. Picking a fix is the user's explicit request, so
   `go` may download modules as it always does.
7. **Words and layout.** The popups' layout and wording are our own (clean-room), in English and
   Russian.

## Consequences

- GoGarden contacts the module proxy and the two vulnerability databases while a go.mod is open,
  unless the user turns the checks off; the ADR and the user guide say so. This is the first
  network use that is on by default for a language feature; it is visible in the settings, limited
  to public data, and never sends private module names.
- Any language or plug-in can now offer problems with quick-fixes and declared settings without
  platform changes; plug-ins (WebAssembly) still need an ABI for it (ADR-0056 does not have one yet).
- Tests use a local HTTP server that plays the proxy and the databases; no test touches the network.
