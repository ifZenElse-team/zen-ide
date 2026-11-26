workbench-deps-group = Dependencies
workbench-deps-own-repositories = the project's own repositories
workbench-deps-search = Search for libraries in repositories
workbench-deps-search-description = The Dependencies tool window and completion in build files find libraries in the Maven Central search index and read their versions from maven-metadata.xml.
workbench-deps-check-updates = Check for newer versions of dependencies
workbench-deps-check-updates-description = Reads maven-metadata.xml of each declared dependency from the module's repositories, to show newer versions.
workbench-deps-check-vulnerabilities = Check dependencies for known vulnerabilities
workbench-deps-check-vulnerabilities-description = Asks OSV about each dependency that a public repository lists, to mark vulnerable versions and offer the nearest safe one.
workbench-deps-no-module = { $module } is not a module of this project; dependencies.project.list names them.
workbench-deps-reload-untrusted = The project is not trusted, so it was not loaded again; use Load Changes after trusting it.
workbench-deps-already = { $dependency } is already declared with this scope.
workbench-deps-not-declared = { $dependency } is not declared in this module.
workbench-deps-managed-outside = The version of { $dependency } is managed outside this project, so it cannot be changed here.
workbench-deps-exclude-unsupported = Excluding a dependency is not available yet.
workbench-deps-vulnerable = { $count ->
    [one] This version has 1 known vulnerability
   *[other] This version has { $count } known vulnerabilities
}
workbench-deps-outdated = Newer version is available: { $version }
workbench-deps-fix-upgrade = Upgrade to { $version }
workbench-deps-fix-safe = Upgrade to unaffected version { $version }
workbench-deps-latest = latest
workbench-deps-changed-outside = { $file } changed on disk while this edit was being prepared, so nothing was written to it. Try again.
