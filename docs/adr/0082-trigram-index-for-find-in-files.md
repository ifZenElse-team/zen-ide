# ADR-0082: A trigram index for Find in Files

- **Status:** Proposed
- **Date:** 2026-10-20
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md) (every capability is a command)

## Context

Find in Files (`search.text.*`, `zen_index::FileIndex::search_text`) read every file of the
project on every search: on a large project the first results waited for thousands of files to be
read from disk. An index of the text's trigrams lets a search read only the files that can match;
the format here is our own.

## Decision

1. **What is indexed.** Every file of the file index (`.gitignore`, the built-in folders such as
   `.git`, `target` and `node_modules`, and the project's excluded folders and name patterns are
   left out, as for the file names), when it is UTF-8 text of at most 2.5 MB without a zero byte in
   its first 8 000 bytes. Other files are recorded as **skipped**: they are never searched, as
   before.
2. **The format, in memory only.** A file's text is folded to bytes: ASCII letters in lower case,
   the Kelvin sign as `k` and the long s as `s` (the only other letters that match an ASCII letter
   regardless of case; a test checks every Unicode letter), everything else as its UTF-8 bytes.
   Every three consecutive folded bytes are a trigram, a 24-bit key. The index maps each trigram
   to the ascending ids of the files that contain it (`u32`), each stored as its distance from the
   previous id in LEB128 (one byte for most entries), and each file's relative path to its id. Nothing is written to disk; the index is built again when the project opens.
3. **Building.** After the file names are walked, a worker thread reads the files with up to eight
   threads and reports its progress every 500 files; the window shows it as the background task
   "Indexing the project" with a progress bar and "N of M files" in the status bar. Quitting
   does not ask about it: the index is built again when the project opens. Dropping the
   `FileIndex` (another project, other exclusions) stops the build.
4. **Updates.** `FileIndex::refresh`, which the file watcher calls through `file.index.refresh`,
   forgets the changed files and folders at once and reads the files that still exist again in
   the background. A read that started before a later change of the file (or of a folder above
   it) is dropped, so a slow read never brings back old text. A changed file's id is not reused:
   a new one is given and the old one dies; the lists drop dead ids once they outnumber the files.
5. **Answering a query.** A plain query needs its whole text; a regular expression needs the
   literal runs it cannot match without, combined with "and" (a sequence) and "or" (alternatives).
   A regular expression with flags (`(?i)`, `(?x)`...) or that cannot be read needs nothing. When
   case is ignored, only trigrams of ASCII bytes are used. The candidates are the files that
   contain every trigram of what is needed; files the index does not know yet (still being built,
   or changed and not read again) and open files (searched in their editor text) are always
   searched. A need without a trigram (`.*`, a text shorter than three bytes) searches every file.
   The matches are then found in the candidates exactly as before, so the results are those of a
   full scan; a property test compares the two on random files and queries.
6. **Agents** read the state with `search.index.status` (ADR-0033).

## Limits

- Memory grows with the text: about one byte per distinct trigram of each file, plus the map of
  trigrams and the paths. Measured on 2026-10-20 over 36 911 files holding 580 MB of Rust source:
  45 million entries in 48 MB of lists, about 75 MB in all (13% of the text), built in 9 s in a
  release build. Dead ids stay in the lists until they outnumber the live files, so a change of
  most files can hold up to twice that until the lists are compacted.
- Files over 2.5 MB, binary files and files that are not UTF-8 are not indexed and not searched.
- The index is as current as the file watcher: a change on disk is searched in full until its
  file is read again.
- Symbols and words (Find Usages) are other indexes, for later.

## Consequences

- A query with a literal on a 100 000-file project shows its first results in about 25 ms in a
  release build (the test asks for under 100 ms; docs/performance.md).
- Opening a project reads its files once in the background; the file names are ready first, so
  Go to File does not wait for the text.
