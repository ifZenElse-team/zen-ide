# Code Quality Tools (ADR-0136)
code-quality-none = No linters or formatters: install the ESLint, Prettier or Biome plug-in, which the IDE offers when a project uses the tool.
code-quality-automatic = Automatic { $tool } configuration
code-quality-disabled = Disable { $tool }
code-quality-not-found = { $tool } is not installed
code-quality-in-project = { $tool } runs from the project's node_modules
code-quality-installed = { $tool } is installed
code-quality-fix-on-save = Run { $tool } fixes on save
code-quality-formatter-found = { $tool } package: { $path }
code-quality-formatter-missing = { $tool } is not in the project's node_modules: run npm install
code-quality-on-reformat = Run on Reformat Code action
code-quality-on-save = Run on save
code-quality-files = Run for files: { $extensions }
code-quality-bad-mode = Unknown mode { $mode }: use on or off
code-quality-page = Code Quality Tools
workbench-format-formatter-failed = { $tool } could not format the file: { $error }
workbench-format-formatter-stopped = the formatter stopped without an answer
workbench-format-formatter-timeout = it did not finish in time
