# ADR-0246: File encodings, and writing `.idea/encodings.xml`

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md) (`.idea` is read,
  never written), [ADR-0123](0123-mark-directory-as-excluded.md) (the first exception),
  [ADR-0002](0002-open-source-clean-room-pure-rust.md) (no C)

## Context

A file in another encoding than UTF-8 opened read-only, a UTF-8 byte order mark stayed in the text as
U+FEFF, and `addBOMForNewFiles` was read but never used. The requirement: the File Encodings
settings are kept in `.idea/encodings.xml`, with the project settings we read (ADR-0051).

## Decision

1. **`encoding_rs` is the codec.** It is pure Rust, already in the workspace through `zen-vcs`, and
   passes `cargo deny` (`(Apache-2.0 OR MIT) AND BSD-3-Clause`). `Charset` keeps EditorConfig's five
   values and adds every WHATWG encoding it can write, plus strict US-ASCII. ISO-8859-1 stays our
   own strict decoder, because WHATWG makes that label mean windows-1252, which would change bytes.
   UTF-16 stays our own, because `encoding_rs` does not encode it. UTF-32 is not in the set: such a
   file stays read-only, as does any bytes that are not text in the file's encoding.
2. **Order of resolution.** The byte order mark, EditorConfig's `charset`, the nearest entry of the
   Path/Encoding table, Project Encoding, Global Encoding, UTF-8. The mark goes first, as
   required: it is a fact about the file, a setting is a guess. A `charset` of `latin1` no longer
   reinterprets a file that starts with a UTF-8 mark.
3. **`.idea/encodings.xml` is a second exception to ADR-0051's "never written".** Only an explicit
   request writes it: Settings → Editor → File Encodings, `project.encodings.set`, or a chosen
   encoding in the status bar. `SettingsProvider` gets `set_encodings`; `zen_jb_import::IdeaProject`
   implements it. Only the `Encoding` component changes: its other attributes and entries the reader
   cannot resolve (`jar://`) stay, the rest of the file stays byte for byte. Automatic writers keep
   refusing `.idea` (`guard::refuses_foreign_settings`).
4. **Global Encoding is an IDE setting** (`editor.globalEncoding`), as it is not the project's.
5. **`Create UTF-8 files` keeps its three values** (`NewFileBom`), since saving must not turn "with
   BOM under Windows" into what it means on this machine.
6. **A change of encoding never loses anything silently.** `file.encoding.set` without `action` makes
   a change only when the text and the bytes stay the same; otherwise it answers what is possible and
   changes nothing, and the UI asks Reload or Convert. Reload is refused over unsaved edits, Convert
   when the encoding cannot hold a character. A save with such a character is refused, as it was for
   `latin1`, with the character, its line and column.
7. **BOM commands work on the bytes on disk,** so unsaved edits stay unsaved, and leave a Local
   History entry.

## Consequences

- JetBrains IDEs and Zen IDE share one encoding table, and a project without `.idea` gets the folder
  when the user makes a choice that has to be kept.
- Closed files keep their limits: Find in Files reads them as UTF-8, and Replace in Files and Line
  Separators leave non-UTF-8 ones alone.
- The status bar label `UTF-8 BOM`, and the wording of the Reload and Convert dialog, are still to
  be confirmed.
