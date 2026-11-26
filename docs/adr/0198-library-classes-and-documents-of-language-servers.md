# ADR-0198: Library classes, and documents a language server shows under its own URIs

- **Status:** Proposed
- **Date:** 2026-11-03
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first),
  [ADR-0148](0148-java-plugin-and-eclipse-jdt-ls.md) (Java through JDT LS),
  ADR-0195 (the build model, proposed), AGENTS.md golden rule 10

## Context

A Java project uses classes it has no sources of: the JDK's and its libraries', packed in jars.
External Libraries listed only SDKs and dependencies that are folders (Go's module cache, Cargo's
registry), and every document of the editor was a file on disk. The Eclipse JDT Language Server
answers Go to Declaration into such a class with a `jdt://` URI, and gives the class's text
(its sources when it finds them, otherwise a stub of its declarations) through its own request,
`java/classFileContents`, when the client says it supports it; without that support the jump went
nowhere. Other servers do the same with their own schemes. The person must be able to open any
class the project uses, read its sources when they exist, and never have the network used without
asking.

## Decision

1. **A server declaration may name virtual documents:** `virtualDocuments` gives the URI schemes,
   the request that answers a URI's text (`{ "uri": ... }` in, the text out) and the extension it
   is shown with. When a navigation answer points at such a URI, the IDE asks for the text, writes
   it into the IDE's cache (`caches/libraries/<scheme>/<hash>/<name>`), opens that file read-only
   and maps its path to the URI both ways (`zen_lsp::position::map_uri`): every request about the
   document carries the server's own URI, so navigation goes on from inside it. The Java plug-in
   declares `jdt` with `java/classFileContents` and turns `classFileContentsSupport` on.
2. **Library documents are read-only files in the cache, marked by kind:** sources of a library or
   a decompiled view. Their reason to be read-only is not the file's permission, so they cannot be
   made writable. A document the IDE wrote without a server URI is never sent to a language server.
3. **A library may be a jar.** A model provider lists a library with an optional `label` and an
   empty `group` puts it right under External Libraries. A jar is unpacked once, in the background,
   into the cache: its folders and files as they are, its classes as empty placeholders; the JDK's
   `lib/src.zip` is unpacked whole. Opening a `.class` file (a placeholder, or any class file) shows
   the class's sources from the library's `-sources.jar` when there is one, else a decompiled view
   that the IDE writes itself from the class file: the declarations, without code.
4. **The JVM library list comes from a replaceable source.** Today: Maven's resolved tree when it
   is loaded; Gradle's resolved classpaths, which an init-script task (`zenLibraries`) prints with
   their jars when the build is reloaded; else the dependencies the `pom.xml` files declare and the
   `group:artifact:version` strings the Gradle scripts declare, each found in the local Maven
   repository or Gradle's cache. A provider's generation (`ModelProvider::generation`) tells the
   project model to read the list again when such a source has loaded.
   ADR-0195's build model, once accepted, replaces these sources without changing the provider.
5. **Sources are downloaded only by the build tool, only when asked:** `mvn dependency:sources`, or
   a Gradle init-script task that resolves the sources artifacts; from the decompiled view's banner
   or `java.libraries.downloadSources`. Automatic download follows the build tool's setting when
   there is one.

## Consequences

- Go to Declaration into the JDK and libraries works, and works on from inside them.
- The cache holds unpacked jars (placeholders keep it small) and the documents opened; it can be
  deleted at any time.
- A library class opened from External Libraries is a file the server does not know: its sources
  read, but navigation from inside it needs the server's URI, which only a jump gives.
- The decompiled view is our own and shows declarations only; a fuller decompiler is a later choice.

## Alternatives considered

- **Documents without files.** Every document would need a second kind of identity through the
  workbench, search, history and the UI; a cache file with a URI mapping keeps them all working.
- **Browsing jars inside the tree without unpacking.** The Project tool window, search scopes and
  Select Opened File all work on folders; unpacked placeholders reuse them.
- **The language server's classpath as the list.** It needs the server running and the import
  finished, and gives paths without coordinates; it stays a possible source.
