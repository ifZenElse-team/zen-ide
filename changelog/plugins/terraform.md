# Terraform plug-in changelog

Every release of the Terraform plug-in (`plugins/terraform`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-13

### Added

- The first release, for `.tf`, `.tfvars` and other HCL files and `.terraformrc`; DevopsDaruma
  comes with it, and every other IDE offers it from the Marketplace.
- Highlighting of block types, labels, attributes, numbers, constants, functions, references
  (`var.`, `local.`, `module.`, `data.`...), comments, strings with escapes and `${...}` and
  `%{...}` templates, and heredocs.
- Folding of blocks, lists, heredocs and comments, and the blocks in the Structure view.
- The Terraform Language Server (terraform-ls 0.39.1) for problems, completion, navigation and
  formatting, found on `PATH` or downloaded from releases.hashicorp.com when you click Install.
- `terraform init`, `validate`, `plan` and `apply` from the run icon beside a module's `terraform`
  block. `apply`, `destroy`, `import`, `state` and the other commands that change state are yours
  alone: the IDE shows the whole command and asks you every time, and AI agents never start them.
