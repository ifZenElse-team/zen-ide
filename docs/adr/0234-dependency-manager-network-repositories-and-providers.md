# ADR-0234: What the dependency manager asks, from whom, and how providers plug in

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0195](0195-dependency-manager-and-the-build-model.md) (the dependency manager),
  [ADR-0059](0059-go-mod-dependency-checks.md) (network checks of dependencies),
  [ADR-0196](0196-load-changes-and-the-sync-view.md) (Load Changes),
  [ADR-0205](0205-trusted-projects.md) (trusted projects),
  [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first), AGENTS.md golden rule 10

## Context

ADR-0195 decided the dependency manager and said that it makes read-only GET requests. Building it
raised questions it left open. OSV answers about a package only to a POST. A `pom.xml` is project
content, and it names repositories and, through their ids, the passwords of `settings.xml`. A
version in a `pom.xml` may be managed by a parent or a BOM that is not on this computer. And the
Maven and Gradle code of the workbench is native while plug-ins are WebAssembly without an ABI for
this.

## Decision

1. **OSV is asked with one POST.** `POST api.osv.dev/v1/query` with the package and ecosystem
   `Maven` is the read-only query OSV offers per package; it is the only request that is not a GET.
   It gets every advisory of the package at once, and the IDE decides which versions are affected
   from their ranges, in Maven's version order, so that the nearest safe version needs no more
   requests. It sends `group:artifact` and nothing else, and only for an artifact that a repository
   without credentials lists: a private library's name never leaves.
2. **Repositories follow trust.** The repositories `settings.xml` gives, its mirrors and Maven
   Central always apply. Those a project's own build files name are contacted only once the project
   is trusted, so opening a project cannot point the IDE at a host, or at a password by naming a
   server's id. Credentials go only to the repository with their id, are not sent after a redirect
   to another host, are never shown or kept with the answers, and an encrypted password is not used.
   Gradle's credentials are not read: a Gradle repository is asked without them. A plain `http://`
   repository is not asked, except on this computer.
3. **Parents and BOMs are read to tell what is managed.** The local repository first; otherwise the
   POM is downloaded from the same repositories, which adds a `.pom` to what ADR-0195 lists. Without
   it, a dependency simply shows no managed version.
4. **Three declared settings, off by default (rule 10)** on the Build Tools page: `dependencies.search`,
   `dependencies.checkUpdates` and `dependencies.checkVulnerabilities`, each naming its hosts. The
   vulnerability check reads `maven-metadata.xml` too, for rule 1 and for the safe version. Work
   offline of the Maven or Gradle settings turns all three off.
5. **Providers are a trait the platform selects by plug-in id.** `DependencyProvider` gives the
   modules, declared dependencies with where each version is written, repositories, version sites
   and completion contexts in a build file, the edits as new file texts, and the reload. A provider
   applies only where the plug-in with its id supports the project, so the platform names no build
   system in its logic. Maven's and Gradle's are native, like the rest of their code; a WebAssembly
   provider waits for the ABI of ADR-0056. `DependencyChange::Exclude` is the hook of the analyzer,
   a later step; both providers refuse it for now.
6. **The order of versions is Maven's documented one**, for Gradle too. A pre-release is offered
   only for a dependency already on one, a flavour such as `-jre` only to the same flavour, and a
   version that starts with a date only to one that does. These are the rules behind "latest stable"
   and "newer".
7. **Keeping answers.** In `<data>/caches/dependencies/` per URL with their validators, fresh for an
   hour; a failure is retried after five minutes, and what was kept is shown meanwhile. The modules
   are read again only when a file they were read from changes, because the tool window polls on the
   UI thread and a BOM can list thousands of versions. All network work runs on background workers;
   commands wait only when given `waitMs`.
8. **A quick fix writes the saved file and relies on Load Changes.** It runs outside the session, so
   the project loads again as after any change made outside the editor (ADR-0196). The tool window's
   commands reload themselves, unless the project is not trusted.

## Consequences

- The manager works for private repositories without leaking their names, and a hostile project
  gets no request from the IDE to a host of its choosing until the person trusts it.
- Gradle projects whose repositories need credentials show no versions from those repositories.
- An artifact only an untrusted project's own repository lists shows no versions until it is trusted.
- Search ranks by the index's own version count, a stand-in for popularity; the count is never shown
  as a version.
