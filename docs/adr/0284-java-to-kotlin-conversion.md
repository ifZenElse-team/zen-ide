# ADR-0284: Java to Kotlin conversion is our own, in the Kotlin plug-in

- **Status:** Accepted (the owner, 2026-11-13)
- **Date:** 2026-11-13
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (capabilities are plug-ins),
  [ADR-0280](0280-clean-room-process.md) (the clean-room process), [ADR-0031](0031-plugins.md)
  (the plug-in ABI), [ADR-0033](0033-ai-access-to-every-api.md) (every capability is a command)

## Context

People who move a project from Java to Kotlin expect the IDE to turn a Java file into a Kotlin file,
and to offer the same when Java code is pasted into a Kotlin file. The requirement is that the source
and the licence of the converter be decided first: an open-source converter, or our own.

There is no converter we may use as it is. The ones that exist are part of other IDEs: they run only
inside those IDEs, and the clean-room rules forbid reading their code (rule 2). A language server
could offer conversion as a code action, but neither Kotlin language server the Kotlin plug-in
declares does.

## Decision

1. **Our own converter**, written from the Java Language Specification and the Kotlin language
   specification, in Rust, under the project's licence. It lives in the **Kotlin plug-in**
   (`plugins/kotlin/src/convert/`), since turning code into Kotlin is a Kotlin capability; it has its
   own Java tokenizer and parser and needs nothing installed.
2. **What it does is our design**, written in the user guide: declarations, statements and
   expressions are carried over to their Kotlin forms; what it cannot carry over is kept as the
   original text in a comment and reported as a problem with its place in the Java text, so nothing
   is lost silently. It works on one file's text, without the project's types, so where Java's meaning
   depends on types it does not know (identity `==` on objects, numeric widening) it chooses the
   common case and says so in the guide.
3. **A new optional plug-in export, `zen_convert`** (ABI v1, looked up by name as the others):
   input `from` as a little-endian `u32` length and its UTF-8 name (the source file's extension,
   `java`), then the text; result UTF-8 JSON `{"text": str, "problems": [{"start": u32, "end": u32,
   "message": str}]}`, byte offsets into the input text. A second export, `zen_convert_from`, answers
   the source extensions the module converts from (comma-separated, read once when it loads). The
   platform asks the plug-in of the target file type, so any plug-in can convert into its language.
4. **Every part is a command** (ADR-0033): `file.convert.toLanguage` converts a file into another
   language beside it (and moves the original to the Trash, kept by Local History), and
   `editor.convert.text` converts a piece of text and returns it. The menu item **Code → Convert
   Java File to Kotlin File** and the offer when Java is pasted into a Kotlin file dispatch them.

## Consequences

- Converted code is a start, not a guarantee: the result has to be built and read. The guide says
  what is converted, what is reported, and what to check.
- The converter grows with tests that hold its behaviour, each written from the specifications.
- Other conversions (Kotlin to Java is not planned) can use the same export.

## Alternatives considered

- **Running another IDE's converter.** It cannot run outside that IDE, and its code may not be read
  here; rejected.
- **A language server's code action.** None is offered by the servers we ship; it can be added beside
  ours when one appears.
