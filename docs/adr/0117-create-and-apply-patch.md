# ADR-0117: Create Patch and Apply Patch

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0047](0047-version-control-gitoxide-and-git.md), [ADR-0064](0064-undo-git-operations.md),
  [ADR-0068](0068-changelists-and-shelf.md), [ADR-0072](0072-merge-conflict-editor.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

The requirement is patches in the IDE: Create
Patch from local changes, a commit of the Git Log and shelved changes, to a file or the clipboard,
with a base path, reversed and in an encoding, written as a git-format unified diff; Apply Patch
from a file, the clipboard or a `.patch` dropped onto the editor, with every file's diff, the
patch's paths mapped onto the project (a base folder, leading folders stripped), a target
changelist, Import to shelf, the commit message and author of a patch made from a commit, and hunks
that do not apply reported and merged.

Questions this decision answers: who reads, changes and writes patches (git or our own code); how a
patch is applied and what happens to a hunk that does not apply; how paths in an untrusted patch
are kept inside the project; what "encoding" means for a patch; where the author and message go.

## Decision

1. **Our own patch model, git for what git knows best.** `zen_vcs::patch` (pure Rust, every build)
   reads patches (git's `diff --git` with renames, copies, modes and `GIT binary patch` data, the
   mail header of `git format-patch` with RFC 2047 words, and plain unified diffs with timestamps
   and `Index:` lines; at most 64 MiB, a mode that is not six octal digits ignored), changes them (strip leading folders, put them under a folder, make them
   relative to a folder, reverse, the two blocks of binary data swapped) and writes them in git's
   format. Reading never panics and never trusts a count: hunks are read line by line, a hunk that
   ends early is an error, at most 100 000 files or hunks. The changes themselves come from git:
   `git diff --cached --binary --full-index -M` against a temporary index for local changes (as the
   shelf, ADR-0068), `git show --diff-merges=first-parent` for a commit, the shelf's own patch for
   shelved changes, always with explicit `a/` and `b/` prefixes whatever the user's configuration.
2. **Git format with a mail header.** Every patch made starts with an mbox header: the commit's
   id, author, date, subject and body for a commit; the user's identity (`git var
   GIT_AUTHOR_IDENT`), now and the changelist's or the shelf's name for the others. So `git apply`
   and `git am` both take every patch the IDE writes. The clipboard gets the text as UTF-8; a file
   gets it in the chosen encoding, line by line (a line that is not UTF-8, the bytes of a file in
   another encoding, is kept as it is). Reading a patch decodes it from the chosen encoding (UTF-8
   by default) first.
3. **Applying is our own, conflicts are git's.** Each text file's hunks are applied in order where
   their old lines are, searched outwards from where the header puts them (the offset carried to
   the next hunk). The first try is `git apply`'s own: the whole context byte for byte, and a hunk
   without context at the start or the end of the file only there. Then a
   hunk is tried anywhere, with trailing whitespace and line breaks ignored (its new lines then take
   the file's line breaks, so a CRLF file stays CRLF), and with up to two context lines left out
   at each end; such a hunk is reported as approximate (`approximate` of `vcs.patch.read`, a note
   in the dialog), never applied silently. A hunk whose last line has no line break applies only
   at the end of the file. The search goes through an index of the file's lines, so thousands of
   hunks that do not apply stay cheap. The file's own context lines are kept. Binary changes are applied by `git
   apply --binary`; modes follow the patch. A file whose hunks do not all apply is merged three-way:
   from the blob the patch names (`index` line) when the repository has it, which is git's
   `apply --3way`; otherwise the hunks that did not apply are put where they were expected, their
   old lines as the common ancestor and their new ones as the patch's side. A clean merge is
   applied; one with conflicts leaves the file conflicted the way git does (stages 1, 2 and 3 in the
   index, conflict markers in the file, `Applying::Patch`), so the merge conflict editor of
   ADR-0072 resolves it and the window says so in its conflicts dialog. A rename or a copy onto a
   file that is already there with other content is `exists` (git refuses it) and is left to merge
   as an added file that exists. A clean apply is recorded for Undo (ADR-0064); one that leaves
   conflicts is not, as an unshelve. Local History keeps every touched file's content before and
   after the apply ("Apply Patch"), and an open document takes the new text as one undo step.
4. **Paths stay inside the project.** After stripping, every path is checked before anything is
   read or written: no absolute path, no drive letter, no backslash, no control character, no
   `..` and no `.git` part in any spelling a file system takes for it (any case, the characters
   HFS+ ignores, trailing dots and spaces, `git~1`), so a patch never writes hooks or the
   configuration. A rename's `rename to` must agree with the header. A refused path fails the whole
   patch with its name. When applying, a file whose path goes through a symbolic link, or is one,
   fails (`failed`, with the reason), as `git apply` refuses what is "beyond a symbolic link": a
   link to `.git` inside the project must not open a way in. A patch that makes or changes a
   symbolic link or a submodule (mode `120000` or `160000`) fails too, so a patch never creates a
   link pointing anywhere; a patch that makes a file executable or no longer executable says so
   (`executable`, a note in the dialog). Paths of a patch made by the IDE come from git itself and
   keep every name the repository has (a tab, quotes).
5. **The window shows what the commands say.** `vcs.patch.read` tells how each file would apply
   (`clean`, `merged`, `conflict`, `binary`, `missing`, `exists`, `failed`, with the hunks that do not
   apply) without writing; `vcs.patch.diff` gives a file's diff with the conflict lines;
   `vcs.patch.apply` applies into a changelist (made when it does not exist); `vcs.shelf.import`
   puts the patch on the shelf instead (its files' versions for the preview are worked out the same
   way); `vcs.patch.create` makes a patch. A patch with a mail header fills the commit window's
   message and author in (`vcs.commit.draft`); `vcs.commit.create` takes `author`.
6. **Commands and actions for everyone** (ADR-0033): the commands above, and the actions
   `view.vcs.createPatch`, `view.vcs.copyPatch` (Copy as Patch to Clipboard),
   `view.vcs.applyPatch` (an agent gives `path`; a person picks the file in a system dialog) and
   `view.vcs.applyPatchFromClipboard`; the Git Log's commit menu has `createPatch`
   (`vcs.workspace.commitAction`). The clipboard is the session's (`file_clipboard`), so tests and
   the headless IDE never touch the system's.

## Consequences

- Patches made by the IDE apply with `git apply` and `git am` elsewhere; patches from GNU `diff`,
  `git diff`, `git format-patch` and other IDEs apply here, also onto files that moved on.
- Where `git apply` refuses a patch and the IDE applies it, the dialog and `vcs.patch.read` say
  which hunks applied only approximately; symbolic links and submodules in a patch are left for
  `git apply` on the command line.
- A file that is not UTF-8 applies as long as its hunks match byte for byte; merging it needs text,
  so a hunk that does not apply to it fails the file instead of leaving it conflicted.
- The conflict markers' labels are the window's language ("Current", "Patch").
- `encoding_rs` becomes a direct dependency of `zen-vcs` (MIT or Apache-2.0, and BSD-3-Clause for
  its data; pure Rust; gitoxide already depends on it).

## Alternatives considered

- **Only `git apply`.** It would need the patch's paths rewritten anyway (strip, base folder), it
  gives no per-hunk report or preview without writing, and without blob ids a failed hunk is only a
  `.rej` file, which the conflict editor cannot open.
- **Plain unified diffs without a header.** Simpler, but `git am` refuses them and the author and
  message of a commit would be lost.
- **Decoding every file in its own encoding before applying.** Correct for mixed encodings, but the
  IDE does not keep per-file encodings yet; byte-wise application covers files in one encoding.
