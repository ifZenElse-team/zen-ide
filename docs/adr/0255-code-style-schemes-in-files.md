# ADR-0255: Code style schemes, the Project scheme in `.idea` and the formatter's exclusions

- **Status:** Proposed
- **Date:** 2026-11-06
- **Relates to:** [ADR-0053](0053-fonts-ui-scale-and-code-style.md), [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Editor → Code Style had a Scheme combo that only said whether the code style a project keeps in
`.idea/codeStyles` wins. The page offered neither the General fields that belong to a scheme (Hard
wrap at, Wrap on typing, Visual guides) nor the Formatter tab, and nothing wrote a scheme: the Project
scheme could be read and never edited, and there were no IDE schemes besides the one in
`settings.json`. ADR-0053 keeps the code style per language in `IdeSettings.codeStyle`.

## Decision

1. **`zen_project::settings` holds the neutral model.** `CodeStyleGeneral` is the part of a scheme
   that belongs to no language, `SchemeFile` is a whole scheme as a file keeps it (the styles of its
   languages, with `default` as the scheme-wide margin, guides and other file types, plus the
   options), and `SettingsProvider` gains `write_code_style`, `code_style_text` and
   `parse_code_style`. `zen-jb-import` implements them; the workbench only knows the trait.
2. **The Default (IDE) scheme stays in `settings.json`** as `codeStyle` and `codeStyleGeneral`, in the
   IDE's data folder. Copies (Duplicate…, Copy to IDE…, Import Scheme…) are `codeStyleSchemes` in the
   same file, and `codeStyleCopy` names the one in use. They are not XML files in a `codestyles`
   folder, so that `set_settings` is the one way to change them and agents need no second API.
3. **The Project scheme is written to `.idea/codeStyles/Project.xml`, with
   `codeStyleConfig.xml` turning it on (`USE_PER_PROJECT_SETTINGS`).** The writer changes the options
   the IDE manages and keeps everything else in the file byte for byte (other languages' settings,
   language-specific elements, comments); a file it cannot parse is reported and left alone.
4. **Nothing writes `.idea` unasked.** `codeStyleScheme` stays `project` by default, and a project
   without a scheme keeps taking edits into the IDE's scheme. A project gets files only when the user
   picks Project in the combo or runs Copy to Project…, and the files are written on Apply or OK.
5. **The choice of scheme stays an IDE-wide setting** (`codeStyleScheme`), as ADR-0053's Quick Switch
   Scheme uses it; `USE_PER_PROJECT_SETTINGS` is only written as on, when the Project scheme is
   written.
6. **Do not format is applied at the entrance of Reformat Code, Optimize Imports and the save
   actions.** A pattern with a `/` matches the project-relative path, any other the file name, and
   `*` every file; the commands answer `excluded: true`.
7. **Formatter markers are applied to the result, not sent to formatters.** Text from a line with
   `@formatter:off` to a line with `@formatter:on` is restored after a formatter ran, whichever
   formatter it was, as one undo step. If a formatter changes the number of marked regions the file
   stays as it was. SQL, formatted by the IDE as edits per statement, drops the edits that touch a
   marked region.
8. **A right margin and a continuation indent are used.** The margin comes from `.editorconfig`, then
   the project's scheme, then Hard wrap at. Enter after `(` or `[` takes the continuation indent where
   a source sets one, and the indent otherwise.

## Consequences

- `IdeSettings` gains `codeStyleGeneral`, `codeStyleCopy` and `codeStyleSchemes`; an old
  `settings.json` loads as before.
- The agents' commands are `project.codeStyle.get`, `.set`, `.export` and `.import`, and `set_settings`
  for the IDE's schemes. Reset, Rename and Delete of a copy are plain edits of those settings.
- Line separator and Detect indents stay IDE settings instead of following the selected scheme, and
  Enable regular expressions in formatter markers is not offered; a later change can add them.
