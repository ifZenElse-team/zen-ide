# ADR-0124: Associate with File Type and Override File Type

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0108](0108-file-type-registry-and-ignored-files.md) (the file type
  registry),
  [ADR-0033](0033-ai-access-to-every-api.md) (commands for agents)

## Context

ADR-0108 gave the IDE one registry of file types whose patterns the user edits, and left out
Associate with File Type and Override File Type. The behaviour to build:

- Associate with File Type… is in File → File Properties and the Project tool window's menu, and a
  file the IDE cannot open shows the same dialog: a pattern (`name.ext` or `*.ext`), then open
  matching files in the IDE as text with the type detected by content or as a chosen type, or in
  the associated application.
- Settings → Editor → File Types lists two service types besides the others: "File type
  auto-detected by file content" and "Files opened in associated applications".
- Override File Type gives single files a type, not their pattern; Revert File Type Override undoes
  it. The overrides are kept per IDE, not per project.
- Text (plain text) has no highlighting and no language help.

## Decision

1. **Two service types of the platform**, declared next to Text in `zen_workbench::file_types`:
   `detected` ("File type auto-detected by file content") and `native` ("Files opened in associated
   applications"). They declare no patterns; the user gives them patterns as any type, in Settings
   → Editor → File Types or through Associate with File Type; associations are per IDE, never read
   from a project, and a plug-in cannot declare these ids (nor `text`). A file whose type is `native`
   opens in the system's application for it (`open` on macOS, `xdg-open` on Linux, `explorer` on
   Windows, given the absolute path as one argument, never through a shell) and not in the editor,
   and only when the user opens that file (`file.document.open`, Associate with File Type's OK):
   reopening a project's tabs (`file.document.open { editorOnly: true }`), navigation, Replace in
   Files and the other ways the IDE opens files itself are refused instead of starting it. An
   executable file, or a link to one, is never handed to the system's opener (`open` would run it in
   the Terminal), so a repository cannot pass a program off as `report.pdf`. A file whose type is `detected` opens as text. Both are known to highlighters,
   language servers and the code style as plain text.
2. **When a file asks how to open it.** `file.document.open` decides by the file's own type
   (override), else the type of its name, else as a picture, else the type of its hashbang. A file of
   no type opens as text when its content is text; when
   its first 8 KiB hold a zero byte it is not text, and the command fails with the code
   `unknownFileType` and the data `{ path, patterns }`. Zero bytes that all fall on one half of the
   byte pairs, or a UTF-16 byte order mark, are UTF-16 text, not binary: it opens decoded and
   read-only, as the IDE cannot write UTF-16 yet. What is not a regular file (a folder, a pipe, a
   socket, a device) is refused before anything reads it, so opening it never blocks. The window answers that code with the
   Associate with File Type dialog, instead of a notification; an agent calls
   `editor.fileTypes.associate`. A file of a type the user chose that is not text opens read-only,
   so that saving never changes its bytes.
3. **The dialog** ("Register New File Type Association") takes a
   pattern, `*.<extension>` first, and one of: Text (with file type detection by content), File
   type: <the list>, Open matching files in associated application. OK runs
   `editor.fileTypes.associate { pattern, type, path }`: the pattern moves to the type as
   `editor.fileTypes.addPattern` moves it (so it is kept in `filetypes.json` and shows in Settings),
   then the file opens as its type now says. File → File Properties → Associate with File Type…
   (`view.associateFileType.open`, the file selected in the Project tool window while it has the
   keyboard, else the active editor's) and the Project tool window's menu open the dialog for any
   file.
4. **Overrides** live in the same registry, by absolute path (`Registry::set_override`,
   `zen_vfs::file_types::set_overrides`), and are kept per IDE in `overridefiletypes.json` next to
   `filetypes.json` (`{ "files": [{ "path", "type" }] }`), written only while there is one. An
   override to a type nobody declares now (a removed plug-in) is kept and not used. The service
   types cannot be given to single files. Everything that knows the file's path asks
   `known_name_at` instead of `known_name`: the editor's highlighting, folding and brackets, the
   code style, language servers, inspectors, the Structure, the icons of the Project tool window,
   the editor tabs, Search Everywhere, Recent Files and Locations, Find in Files, Show Usages, the
   Problems and Commit tool windows and the refactoring preview, and the highlighting of diffs,
   conflicts, Local History and Recent Locations. Only the Git log's files of past commits show the
   icon of the name. A row costs one map lookup more; only an overridden file looks its type up again.
   Renames and moves in the IDE (`file.fs.rename`, cut and paste) carry the override to the new path;
   a delete in the IDE drops it.
5. **Text has no highlighting and no language server**: a file whose type is Text is known as
   `file.txt`, which no highlighter and no server declares. Its server is sent `didClose` for the
   open file and `didOpen` again when the override is reverted; the problems of an overridden file
   that no server serves are dropped, and later ones its old server publishes are ignored.
6. **Commands** (ADR-0033): `editor.fileTypes.associate`, `editor.fileTypes.override`,
   `editor.fileTypes.revertOverride`, `editor.fileTypes.overrides`; `editor.fileTypes.typeOf`
   answers also `overridden`, `highlighted` and `languageServer`; `view.associateFileType.open`
   opens the dialog. The system's application is started through `SystemOpener`, a state of the
   window's session: tests put a fake there, and `ZEN_SYSTEM_OPENER=<program>` makes the IDE start
   that program with the path instead, for end-to-end checks.

## Consequences

- A binary file of no type no longer fails with "the file is not UTF-8 text": it asks how to open
  it. A text file of no type (`LICENSE`, `notes.log`) opens as before, without asking.
- Overrides follow the path: they move with renames and moves made in the IDE, but a file renamed
  or deleted outside it (the Finder, `git checkout`) loses its own type, and that override stays in
  the file until reverted.
- The Override File Type list and the dialog's list show every declared type but the two service
  types; a type of a switched-off plug-in is not offered.
