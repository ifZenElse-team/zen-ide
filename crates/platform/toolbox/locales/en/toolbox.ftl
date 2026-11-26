# Zen Toolbox feature: what each tool is for, and messages. English is the source language.

toolbox-subject-rustronin = Rust
toolbox-subject-gogarden = Go
toolbox-subject-pypetal = Python
toolbox-subject-jvmjizo = for Java and the JVM
toolbox-subject-webwave = The web: TypeScript, JavaScript, HTML and CSS
toolbox-subject-cppcrane = C and C++
toolbox-subject-sharpshuriken = C# and .NET
toolbox-subject-phppagoda = PHP
toolbox-subject-rubyryokan = Ruby
toolbox-subject-sqlscroll = SQL and databases
toolbox-subject-devopsdaruma = DevOps: containers, infrastructure as code, pipelines
toolbox-subject-testtotami = QA: end-to-end and UI tests, API and load testing, test management

toolbox-error-not-initialised = Zen Toolbox is not initialised.
toolbox-error-demo = This is demo data: nothing can be installed or launched.
toolbox-error-no-tools-dir = No tools directory is configured.
toolbox-error-unknown-tool = There is no tool “{ $id }”.
toolbox-error-no-server = No release server is configured.
toolbox-error-not-released = { $tool } is not released yet.
toolbox-error-up-to-date = { $tool } is up to date.
toolbox-error-not-installed = { $tool } is not installed.
toolbox-error-cannot-start = Cannot start { $tool }: { $error }
toolbox-error-no-folder = There is no folder “{ $path }”.
toolbox-error-not-a-folder = “{ $path }” is not a folder.
toolbox-error-unknown-project = No Zen IDE has opened “{ $path }”; choose the IDE to open it in.
toolbox-error-self-up-to-date = Zen Toolbox is up to date.
toolbox-error-no-manifest = The release manifest is not loaded yet.
toolbox-error-no-executable = Cannot locate the Zen Toolbox executable.
toolbox-error-no-build = There is no { $tool } build for this platform.
toolbox-error-busy = { $tool } is already being installed.
toolbox-error-version-installed = { $tool } { $version } is already installed.
toolbox-error-no-version = There is no { $tool } { $version } on this channel for this platform.

toolbox-uninstalled = { $tool } was uninstalled.
toolbox-installed = { $tool } { $version } is installed.
toolbox-install-failed = Cannot install { $tool }: { $error }
toolbox-self-installed = Zen Toolbox { $version } is installed. Restart to use it.
toolbox-self-failed = Cannot update Zen Toolbox: { $error }

toolbox-settings-error-value = “{ $value }” is not a valid value of { $key }. Use one of: { $expected }.
toolbox-settings-error-relative-path = { $key } must be an absolute path, not “{ $path }”.
toolbox-settings-error-write = Cannot save { $key }: { $error }

toolbox-error-no-shell-scripts-location = No directory for shell scripts is set, and the home directory is unknown.
toolbox-error-launcher-directory = Cannot create the shell scripts directory { $path }: { $error }
toolbox-error-launcher-write = Cannot write the shell script { $path }: { $error }
toolbox-error-launcher-remove = Cannot delete the shell script { $path }: { $error }
toolbox-error-launcher-path = The path { $path } cannot be written into a shell script.
toolbox-error-launcher-relative = The shell scripts directory must be an absolute path or start with ~, not “{ $path }”.
toolbox-shell-scripts-failed = Cannot update the shell scripts: { $error }
toolbox-error-no-home = The home folder is unknown, so there is no shell profile to add the line to.
toolbox-error-profile = Cannot add the line to the shell profile: { $error }
toolbox-error-person-chooses = An AI agent cannot do this: a person chooses a version or turns on automatic updates in Zen Toolbox.

toolbox-login-failed = Cannot change launch at login: { $error }
toolbox-login-not-ours = { $path } was not written by Zen Toolbox, so it is left alone.
toolbox-login-io = { $path }: { $error }

toolbox-notice-title = { $tool } { $version } is available
toolbox-notice-body = Open Zen Toolbox to update.
toolbox-updated-title = { $tool } is updated to { $version }
toolbox-updated-body = It updated by itself, as set in Zen Toolbox.
toolbox-update-failed-title = { $tool } could not update to { $version }
toolbox-update-failed-body = { $error }
