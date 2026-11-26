# ADR-0213: Compound run configurations, templates and run configurations in Services

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

The requirement: start several run configurations at once with a Compound configuration, let
each kind of configuration have a template that new ones start from, keep configurations in
folders of the Run/Debug Configurations dialog, and list configurations with their run state in
the Services tool window. Our configurations come from run providers (ADR-0048), are kept in
`.zen/runConfigurations.json`, and a configuration can already start another one before it launches
(the `runConfiguration` before-launch task, ADR-0202). Services showed only HTTP Client responses.

## Decision

1. **A compound is a kind of the run service, not of a provider.** The kind `compound` (provider
   `run`) is always listed after the providers' kinds. Its only option, `configurations`, lists the
   ids of the configurations it starts. Starting it starts each member through the ordinary start,
   in a run of its own, so before-launch tasks, tabs, instances and rerun rules stay the members';
   inner compounds are expanded and a member reached twice starts once. Every run remembers the
   compound that started it (`compound`), which is how a compound is stopped as a whole
   (`run.process.stop { configurationId }`). Debugging a compound debugs the members whose provider
   gives a debug plan and runs the others. No provider changes; `RunProvider` is unchanged.
2. **A cycle is refused where configurations are written.** `run.configuration.create` and `update`
   refuse a compound that reaches itself; the dialog runs the same check
   (`zen_workbench::compound::cycle`) on its draft and blocks saving.
3. **Templates live in the run configurations file**, under `templates`, one `{ kind, common,
   options }` per kind. A template fills what a new configuration leaves out: the dialog's +
   (`run.configuration.new`), `run.configuration.create` (unless `template: false`, which the dialog
   passes because its draft already started from the template) and the configuration of a context,
   before it is matched with the saved ones. Existing configurations never change with a template.
   Templates of other IDEs are not adopted: their shared files have none.
4. **Folders are a key of the configuration** (`folder`), not a list of their own: a folder exists
   while a configuration is in it, under that configuration's kind.
5. **What Services shows is the window's**, kept per user: kinds added
   whole, configurations added one by one and hidden ones (`RunServices`), saved with the window's
   workspace. Run, Debug, Stop and Rerun there are the ordinary commands; Services groups by kind
   only, because no provider exposes other groupings (containers, databases) to it yet.

## Consequences

- Every provider's kinds can be combined in a compound, in every IDE, without code in the provider.
- A compound's own common settings and before-launch tasks are not used; the dialog shows only its
  name and members.
- A `.run` file's `CompoundRunConfigurationType` is adopted with its members found by name, and its
  `folderName` becomes `folder` for every adopted configuration.
- A new grouping in Services (for example by container) needs a provider interface first; it is not
  part of this decision.
