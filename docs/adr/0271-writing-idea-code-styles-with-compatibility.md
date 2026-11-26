# ADR-0271: The Project code style is written to `.idea`, and stays compatible

- **Status:** Accepted (the owner, 2026-11-07)
- **Date:** 2026-11-07
- **Supersedes in part:** [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md), section 4,
  the words "nothing in Zen IDEs writes these formats"
- **Relates to:** [ADR-0255](0255-code-style-schemes-in-files.md)

## Context

ADR-0051 promised that nothing in Zen IDEs writes `.idea`, `.run` or `*.iml`. ADR-0255 writes the
Project code style scheme to `.idea/codeStyles`. The owner's decision (2026-11-07): writing is not to
be forbidden, compatibility is to be kept.

## Decision

1. **`project.codeStyle.set` may write `.idea/codeStyles/Project.xml` and `codeStyleConfig.xml`**,
   through `zen-jb-import`, when asked; nothing writes them unasked (ADR-0255).
2. **Compatibility is the condition.** The files stay in the `.idea` format, under its element and
   option names. What the IDE does not manage stays byte for byte: other options, unknown elements
   and attributes, other languages, comments. A file it cannot parse is reported and left alone, and
   writing the same scheme twice changes nothing. Tests in `idea/code_style_write.rs` hold this.
3. **The rest of ADR-0051 section 4 holds.** Automatic writers keep refusing these files through
   `guard::refuses_foreign_settings`: refactorings and Replace in Files do not touch them.

## Consequences

- A team on a JetBrains IDE and on a Zen IDE can share `.idea/codeStyles`.
- Another writer of these formats needs its own decision and the same condition.
