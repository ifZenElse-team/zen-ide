# Performance

How fast the editor is, how it is measured, and the numbers to keep. Phase 3 ends when editing a
large Go project is pleasant with measured typing latency (ROADMAP.md).

## Measuring

```sh
cargo xtask bench-editor
```

It builds GoGarden in release mode, runs the binary from the target directory that build used
(`CARGO_TARGET_DIR` when set, otherwise `cargo metadata`'s `target_directory`), and opens generated Go files of 1 000, 10 000 and 100 000 lines
(functions with loops, conditions, comments and strings), each in a fresh data folder. For ten
seconds the window types (`ZEN_UI_PROFILE=type`: a character, then Backspace, one key per frame,
into the focused editor) or scrolls (`ZEN_UI_PROFILE=scroll`, down and up as a trackpad); the first
five seconds (start-up and the splash) are left out. The window is shown, so it needs a desktop
session and should run with nothing else busy.

**What is measured** is the time a project window takes to build a frame (`zen_ui::capture`,
`profile_begin` / `profile_end` around `IdeView::show`): handling the input, running the commands,
laying out and highlighting the text, and building the shapes. In typing mode every frame handles
one keystroke, so this is the keystroke-to-frame work. Tessellation and the GPU's drawing come
after it; at 60 Hz the whole frame has 16 ms.

The table shows p50 / p95 / max in milliseconds, the worst two-second report of each run, and the
most lines a keystroke highlighted again (`lexed=` in the report, ADR-0077).
`ZEN_UI_PROFILE` can also be set by hand on any project to watch the same report.

## Budget

- Typing on a 10 000-line Go file: p95 under **16 ms** (`bench-editor` prints a warning when over).
- Scrolling: p95 under 8 ms at any size, so that 120 Hz displays stay smooth.

## Measurements

2026-10-18, GoGarden 0.37.1, MacBook Pro (Apple M-series, aarch64), release build, `cargo xtask
bench-editor`:

| Lines | Typing | Scrolling |
|---:|---|---|
| 1 000 | 1.8 / 2.8 / 4.9 | 0.9 / 2.1 / 3.6 |
| 10 000 | 2.5 / 4.2 / 5.3 | 0.6 / 0.7 / 0.8 |
| 100 000 | 11.0 / 11.8 / 13.5 | 0.8 / 1.0 / 1.2 |

Scrolling a 50 000-line file: 0.9 / 1.2 / 3.2 (with gopls running: 2.1 / 3.6 / 4.7).

GoGarden 0.34 had grown to 85 ms per keystroke on 100 000 lines (8–9 ms on 10 000) and 9.2 ms p95
when scrolling 50 000 lines: features added after the baseline went through the whole text on every
keystroke or copied whole lists on every frame — the foldable regions (read, converted to lines and
sent to the view in full), the run icons (`run.targets.list` tokenizes the file), the occurrences of
the name at the caret, the names doc comments start with, and copies of the fold list, the semantic
tokens and the run targets for each frame. Now:

- What is read from the whole text waits until it has not changed for 0.3 s (`SETTLE` in
  `editor_view.rs`): meanwhile the folds last read are moved
  along with the edits by the document (`Document::foldable_listed`, `editor.fold.list` with
  `current: false`), the run icons stay, and the usages of the name at the caret are hidden (the language server is
  asked for them, `textDocument/documentHighlight`, only once the text settles, and once per caret place).
- The view lists only the folds of the lines around those shown, every folded one, and those
  around the first line listed (`editor.fold.list` with `fromLine`/`toLine` and `enclosing`, a few
  hundred folds however long the file). Each listed block tells the line it sticks with
  (`stickyLine`, from the text of its first line and the header lines above it, at most 16), so the
  sticky lines are picked from that list for each frame, without reading the text.
- The method separators are read once per version of the text, after it settles, and while the
  language server has not answered (`editor.methodSeparators.list`); while typing goes on they are
  moved along with the edits, one pass over them and no lookup in the text for those before or after
  a single edit; each frame only looks up the rows shown, and the line numbers' relative mode only
  counts rows.
- The semantic colours and the doc comment names are worked out for the lines shown only.
- Inlay hints are moved with an edit by one pass over them, only in a file that has some; a line
  without hints is laid out as before, without a copy or an allocation more; the labels' widths and
  the widest line with hints are kept until the hints, the font or the text change.
- Code Vision (`editor.codeVision.list`) is read only once the text settles, for the declarations
  on the lines shown and 40 around them; at most four usage requests are on their way at once, the
  counts and the blame are kept until the text changes, and while typing goes on the rows above the
  declarations move with the edit at the caret instead of being read again. Requests for
  declarations that scrolled out of that window are cancelled, so slots go to what is shown. The
  rows' layout keeps the declarations' rows in order and finds a row at a height by binary search;
  with Code Vision off it holds nothing and the rows are where they were. Git blame itself is
  computed once per repository state; each settled version maps it onto the text with one diff,
  as the change markers do.
- Lists kept between frames are shared (`Arc`), not copied; the Go highlighter looks keywords up in
  a set; `vcs.lineStatus.list` copies no text when its markers are current.

- Soft wraps lay out only the lines shown: each line's rows are cached by its text and the wrap
  width; the other lines' heights are estimated from their length until they are shown, and the
  counts already known move with the edits, so typing lays out the edited line only and scrolling
  the lines that come into view (`ide/soft_wrap.rs`). When a line comes into view above the top
  and its rows differ from the estimate, or the width changes, the scroll offset moves by the
  difference, so that the text on the screen does not jump. A file that does not wrap does no
  wrap work at all: no line is measured, the rows are the plain list of lines, a line's one row
  needs no allocation (`LineWrap::single`), and whether a file wraps is read from the settings
  without copying them; a test counts the layouts.

Typing on a large file was then back where it was, but what remained grew with the file: the editor
view highlighted the whole text again and rebuilt its line index after every edit.

## Incremental highlighting

Since 2026-10-20 ([ADR-0077](adr/0077-incremental-highlighting-with-line-lexers.md)) a Go file is
lexed line by line and a keystroke lexes its own line: the edit is known from the document
(`Document::changes_since`), the lines after it are lexed again only while their state at the
start changes (a `/*` opened lexes the rest of the file), and the editor's line index (line starts
and widths) moves with the edit instead of being rebuilt. The highlighting of a file is kept once
(`zen_workbench::brackets::Highlighters`) for the editor, the bracket commands and the folds, which
now come from the lexer's tokens. Other languages are still highlighted whole after an edit.

2026-10-20, MacBook Pro (Apple M5), release builds of `main` before and after, typing on the files
of `bench-editor` with `ZEN_UI_PROFILE=type` for 15 s (the first 5 left out); the editor view's own
time per frame (its `editor:` report) and the whole frame's p50 (median of the reports) and p95
(worst):

| Lines | Editor view before | Editor view after | Frame p50 before → after | Lines lexed a keystroke |
|---:|---|---|---|---:|
| 1 000 | 0.34 ms (longest 0.97) | 0.27 ms (longest 1.11) | 0.63 → 0.54 ms | 1 |
| 10 000 | 1.05 ms (longest 2.55) | 0.32 ms (longest 1.08) | 1.38 → 0.67 ms | 1 |
| 100 000 | 8.97 ms (longest 10.39) | 0.92 ms (longest 2.38) | 9.90 → 1.94 ms | 1 |

The lexing itself (`cargo test --release -p zen-lang-go -- --ignored --nocapture benchmark_typing`):
a keystroke 0.006 ms on 10 000 lines and 0.026 ms on 100 000, against 1.27 ms and 6.92 ms for
highlighting the whole text.

`cargo xtask bench-editor` the same day: typing p50 / p95 / max 0.5 / 18.3 / 45.4 (1 000 lines),
0.7 / 18.2 / 85.1 (10 000), 1.9 / 2.1 / 4.1 (100 000), against 0.6 / 21.1 / 40.5, 1.4 / 12.9 / 42.4
and 9.7 / 10.6 / 11.9 before. The p95 of the smaller files is not the editor's: a sample of the
window while typing shows it waiting in `write` to gopls's standard input (`SyncServers`: the
`didChange` notifications and inlay hint requests are written on the window's thread, and block
while gopls is busy), which grows the longer typing goes on; on 100 000 lines gopls is still
loading and nothing is sent. Writing to the language servers off the window's thread was the next
step for the 10 000-line budget (below).

## Writing to language servers off the window's thread

Since 2026-10-20 ([ADR-0078](adr/0078-language-server-writer-thread.md)) nothing the IDE sends to
a language server is written on the window's thread.
`zen_lsp::outbox` keeps a queue per server that a writer thread empties into the server's
standard input; the window (and the thread that reads the server, when it answers the server's
own requests) only queues a message and returns:

- **Order** is kept: messages are written as they were queued.
- **Merging**: a `didChange` queued right behind an unwritten `didChange` of the same document
  becomes one notification (the changes of both, in order, at the newer version; from the last
  whole text on). A request cancelled before it was written is taken out of the queue instead of
  being followed by `$/cancelRequest`, and answers `RequestCancelled` at once; the changes on
  either side of it then merge. A change, a save, a change and a save of a document become a
  change and a save (typing a character and deleting it makes the file what is saved again,
  which the workbench tells the server as a save).
- **Bounded memory**: at most 64 MB (estimated from the messages' text) wait for a server. A
  server that lets the queue fill up has stopped reading: the queue is dropped, the requests
  waiting fail, and the server is restarted as after a crash (three times in three minutes at
  most). The same happens when its input cannot be written.

`lsp.servers.list` reports each server's `queued` messages. While `ZEN_UI_PROFILE=type` types a
key per frame (about 1 300 a second) into a 10 000-line file, gopls cannot keep up, yet the queue
stays at about five messages: each frame's change and completion and hint questions merge with or
replace the ones still waiting.

2026-10-20, MacBook Pro (Apple M5), release builds of `main` before and after, the same day:

`cargo xtask bench-editor`, typing p50 / p95 / max:

| Lines | Before | After |
|---:|---|---|
| 1 000 | 1.4 / 14.7 / 42.0 | 1.4 / 1.7 / 2.7 |
| 10 000 | 1.5 / 11.4 / 30.2 | 1.5 / 1.9 / 2.6 |
| 100 000 | 2.7 / 3.4 / 5.9 | 2.7 / 3.2 / 4.5 |

Typing for 30 s into a 10 000-line file of a Go module with gopls running (`ZEN_UI_PROFILE=type`,
its two-second reports from the start; the frames each report counted in brackets):

| Seconds | p95 before | p95 after |
|---:|---|---|
| 2–4 | 0.87 ms (1 846) | 0.75 ms (2 774) |
| 6–8 | 8.99 ms (984) | 0.75 ms (2 691) |
| 10–12 | 13.00 ms (861) | 0.75 ms (2 763) |
| 14–16 | 18.81 ms (692) | 0.76 ms (2 680) |
| 18–20 | 44.94 ms (355) | 0.77 ms (2 721) |
| 22–24 | 50.14 ms (323) | 0.77 ms (2 686) |
| 28–30 | 49.37 ms (313) | 0.79 ms (2 587) |

Before, the window waited in `write` to gopls more the longer typing went on, and drew ever fewer
frames; after, the frame time stays where the editor puts it.

## The completion popup and the folds while typing

Since 2026-10-20 a frame that shows the completion popup or the fold marks while
typing goes on reads neither again:

- **Completion.** `editor.completion.list` keeps the items it matched and sorted until what was
  typed, the server's answer or the error changes, and numbers them (`revision`); the editor asks
  with the revision it holds (`since`) and gets `unchanged: true` without items while they are the
  same, so nothing is matched, copied or sent to the window again. The popup lays out only the
  rows in view (at most eleven, however many items there are), and the selected item's
  documentation is parsed once per item. The server's answer is read with the line starts of the
  text counted once (`zen_lsp::position::LineStarts`), not by going through the text from its start
  for every item's edit.
- **Folds.** While the text has not settled ([`SETTLE`](#measurements)), the editor no longer lists
  the folds (`editor.fold.list`) after every keystroke: it moves those it listed along with the
  edits (`Document::char_edits_since`) and looks up only the lines of the folds after the edit; the
  sticky line moves with its block. They are listed again when the text settles, when a region is
  folded or unfolded (`Document::folds_version`), or when the lines shown leave those listed.
  Moving the carets lists nothing (`editor.fold.reveal` runs only when something is folded).

2026-10-20, MacBook Pro (Apple M5), release builds of `main` before and with this change, typing for 30 s
(`ZEN_UI_PROFILE=type`, a key a frame at 120 Hz) into a 10 000-line file of a Go module with gopls
running, the caret inside a function (line 4 808), its two-second reports:

| Seconds | p95 before | p95 after | Editor view before → after (mean) |
|---:|---|---|---|
| 0–2 | 2.35 ms | 2.20 ms | 0.67 → 0.62 ms |
| 10–12 | 1.61 ms | 1.61 ms | 0.68 → 0.49 ms |
| 14–16 | 2.39 ms | 1.34 ms | 0.91 → 0.38 ms |
| 20–22 | 1.33 ms | 0.97 ms | 0.58 → 0.31 ms |
| 28–30 | 1.61 ms | 1.08 ms | 0.69 → 0.32 ms |

The worst p95 of the run: 2.45 ms before, 2.20 ms after (budget 16 ms). A sample of the window
before showed `editor.fold.list` as the largest part of the editor's frame (sticky lines, line
numbers and JSON of a few hundred folds every keystroke); after, it does not appear while typing.

A harder run the same day, same builds: Go code typed through `press_keys` for 30 s (calls of
`fmt.`, `strings.` and the file's own functions, a pause of a second after `.` and after a word's
first letter, so that gopls answers with lists of up to 200 items and the popup stays open), and
the same code typed a key a frame, as `ZEN_UI_PROFILE=type` types `x`: p95 2.1–2.5 ms before and
2.1–2.2 ms after with the pauses, 2.6–2.9 ms before and 2.2–2.3 ms after a key a frame. The 27–171 ms
of the report did not appear in a release build on this machine.

## EditorConfig on every keystroke

Enter, Tab and ⇧Tab ask for the file's indent unit, and the editor asks for its tab size every
frame; both include what the project's `.editorconfig` files set (ADR-0100). Since 2026-10-22 `zen_workbench::editor_config::EditorConfigs` keeps the parsed files and the result
for each file, and the disk is not touched again until the file watcher reports an `.editorconfig`
(or a second has passed, as a safety net). The workbench test
`every_keystroke_is_answered_from_the_cache_in_a_project_with_hundreds_of_files` builds a project
of 441 `.editorconfig` files, opens a file 41 folders deep, deletes every `.editorconfig` and checks
that 10 000 indent and tab size lookups still give the cached answer, so none of them read the disk.
2026-10-22, MacBook Pro (Apple M-series), debug build: the first lookup took 3 ms, then 14–17 µs a
keystroke for the indent unit and the tab size together; the same loop before this change, which
checked the size and time of each folder's file on every lookup, took 730 µs a keystroke.

## Find in Files

`zen-index`'s test `a_query_over_a_hundred_thousand_files_shows_its_first_results_within_a_hundred_milliseconds`
searches a literal over 100 000 indexed files, twenty of which contain it, and fails in a release
build when the first results take 100 ms or more (`cargo test -p zen-index --release`). On
2026-10-20, MacBook Pro (Apple M-series), it took 22 to 25 ms from the call to the first results;
without the trigram index ([ADR-0082](adr/0082-trigram-index-for-find-in-files.md)) every file was
read from disk first.

## Saving with the On Save options

Saving runs Editor → General → On Save before the file is written
([ADR-0106](adr/0106-on-save-whitespace-options.md)): a line diff of the saved text and the text,
then one replacement per line that loses its trailing spaces. On 2026-10-22, MacBook Pro (Apple
M-series), a 100 000-line Go file with every other line changed: the cleanup took 30 ms in a
release build with Modified lines (50 000 lines stripped) and 47 to 53 ms with All lines (100 000),
writing the file 6 to 14 ms more; a debug GoGarden or RustRonin answered `file.document.save` over
MCP in 0.7 to 2.1 s (Modified lines) and 1.2 to 3.8 s (All lines), depending on how busy the
machine was. Before, each replaced range rewrote the
whole text (10 000 lines: 11.6 s with Modified lines, 20.5 s with All lines, debug build), the
modified lines were matched against every diff hunk (96 s more for 100 000 lines, debug), and the
histogram line diff alone took 7.6 s in a release build where Myers' diff, used now, takes 4 ms. `zen-workbench`'s test
`saving_a_file_of_100_000_lines_takes_its_trailing_spaces_at_once` saves such a file and fails
when it takes 30 s, which only a pass per line reaches.

## Baseline

2026-10-17, GoGarden 0.20, the same machine, release build:

| Lines | Typing | Scrolling |
|---:|---|---|
| 1 000 | 1.5 / 1.7 / 2.0 | 0.8 / 1.1 / 1.4 |
| 10 000 | 2.3 / 3.6 / 4.6 | 0.8 / 1.1 / 1.2 |
| 100 000 | 10.9 / 11.3 / 12.2 | 0.8 / 1.0 / 1.2 |

Run the benchmark before and after a change to the editor and update the measurements when the
numbers move.

## The Git workspace on very large histories

```sh
cargo test --release -p zen-workbench --lib -- --ignored --nocapture benchmark_a_large_history
```

The benchmark (ignored by default) makes a repository of 200 000 commits with `git fast-import`
(`ZEN_BENCH_COMMITS` sets another length): a main line with a short branch merged back every 40
commits, 12 branches, a remote branch, 200 tags, eight authors, a third of the messages with a body.
It opens it as the Git workspace does and reads `vcs.graph.page` while the history is read in the
background, then searches it and asks for the details of a commit half way down.

**Budget** (release build): the first rows in under 500 ms, the whole history in under 5 s, a page
read meanwhile in under 100 ms (the window never waits for the reading), under 1 KB of memory a
commit (the graph's own count, `vcs.graph.page`'s `reading.memory`), a search in under 200 ms and a
commit's details (with the branches and tags that contain it) in under 100 ms. The test fails when
one is over.

**Measurements**, 2026-10-19, MacBook Pro (Apple M5), release build:

| What | Benchmark | GoGarden, the same repository |
|---|---|---|
| First rows | 9 to 30 ms | 21 to 85 ms after the tab opens |
| The whole history | 0.6 to 1.0 s | 0.67 to 1.3 s |
| Memory a commit | 288 to 290 bytes | 288 bytes |
| A page while reading | at most 11 ms | frames p95 1.6 ms, max 9.8 ms while reading |
| Search ("Case 7", 3702 rows) | 39 to 41 ms | "change number 1000": 15 ms |
| A commit's details | 4.7 ms (`git for-each-ref --contains`: 730 ms) | selecting a commit half way down: 120 ms over MCP (870 ms before) |

The history is read by `Repository::read_history` (gitoxide, newest first) into a compact store
(`zen_vcs::history::HistoryStore`) and laid out as it comes (`zen_vcs::graph::Layout`,
`PackedRows`); see [git-workspace.md](git-workspace.md#delivery-order), stage 16.

## Whitespaces, indent guides and the right margin

The editor draws them for the visible rows only (`ui/src/ide/visual_aids.rs`): a row's indent
guides at the indents of the lines above that open its blocks (looked for once a frame for the
first visible row, at most 5 000 lines up, then carried down the rows; a blank row looks at most 200
lines up and down for text), the dots
and tab marks from its own characters, the margin and the visual guides once a frame. The block
the caret is in is found again only when the text or the caret's line changes.

2026-10-22, MacBook Pro (Apple M5), release build, the editor view alone in a 800 × 600 test
window (`cargo test --release -p zen-ui --lib -- --ignored --nocapture typing_with_the_visual_aids`,
with `ZEN_UI_PROFILE=type`): 6 000 keystrokes (`x` and Backspace in turn) at a nested line of a
16 000-line Go file with spaces, tabs and trailing whitespace, every aid off, then all on
(whitespaces of every kind, indent guides, the margin), then off again; two runs:

| Visual aids | p50 | p95 |
|---|---|---|
| Off | 0.25–0.26 ms | 0.35–0.40 ms |
| All on | 0.24–0.26 ms | 0.33–0.34 ms |

After the guides moved to the indents of the enclosing lines (review, 2026-10-22, the same machine
heavily loaded by other builds, one run): off 0.20 / 0.27 ms, all on 0.20 / 0.27 ms, off again
0.22 / 0.30 ms (p50 / p95).

No difference shows within the noise; the longest frames (up to 40 ms) came in either state, from
the machine. The windowed `bench-editor` was not run: it brings its window to the front.

## The Find in Files window

The window used to ask `search.text.results` for every match on every frame (the command copies,
sorts and serialises them), lay out a row for each of them, scrolled into view or not, and read the
selected match's lines for the preview (`search.text.preview`, from the disk for a file not open in
an editor). Now it keeps the results it read and asks again only while the search runs (every
100 ms) or when a new one starts, lays out only the rows in view (`ScrollArea::show_rows`), and asks
for the preview once each time the selected match changes. Frame time with the window
open on 2 000 matches (`fn ` in this repository), `ZEN_UI_PROFILE=idle`, MacBook Pro (Apple
M-series), 2026-10-20, p50 in ms:

| Build | Before | After | No window open |
|---|---:|---:|---:|
| debug | 16.3 | 1.4 | 1.2 |
| release | 3.9 | 2.0 | 1.9 |

`a_frame_lays_out_only_the_results_in_view_and_fetches_them_once` (`zen-ui`, `ide/find_in_files.rs`)
checks that a frame lays out no more rows than fit in the list and that finished results are not
read again.

The window starts a search 150 ms after the query and its options stop
changing, not on every key, and asks for at most one match over its limit (100, Settings →
Advanced Settings → Find in Files). Its preview holds 201 lines around the match and highlights
them again only when their text changes; what is typed there is written when typing pauses for
400 ms. `typing_starts_the_search_once_the_query_rests` checks the wait.

Measured in-process on 2026-10-22 (a real `IdeApp` drawn off screen at 1280×800, 200 frames each,
the window's limit raised to 1 000 and 1 000 matches in one file, the preview open), frame time
p50 / p95 in ms:

| Build | Window closed | Open, idle | Scrolling the list | ↑ / ↓ (the preview follows) |
|---|---:|---:|---:|---:|
| debug | 1.5 / 2.1 | 3.6 / 16.5 | 4.8 / 33.8 | 5.2 / 69.1 |
| release | 0.19 / 0.28 | 0.38 / 0.55 | 0.35 / 0.57 | 0.51 / 0.70 |

## rust-analyzer on a large workspace

How fast rust-analyzer answers on this repository (70 packages, 617 crates with the
dependencies) through a headless RustRonin over MCP, the way agents develop it (milestone
XzgpVBIJ), and the defaults the Rust plug-in gives it ([ADR-0093](adr/0093-rust-analyzer-settings-tuned-and-layered.md)).

**Method.** A copy of the repository (`git archive`), RustRonin started with `--headless
--mcp-port`, rust-analyzer 1.99.1, MacBook Pro (Apple M-series, 10 cores, 16 GB). **Cold**: a fresh
data folder and no `target/rust-analyzer`; **warm**: the same again (its `target/rust-analyzer`
kept). A script, over HTTP MCP: opens `crates/platform/core/src/registry.rs` and waits until
`lsp.servers.list` says rust-analyzer is running and not busy (**ready**, from the IDE's start);
Go to Symbol `dispatch` (`waitMs` 10 000); Find Usages of `Registry::dispatch` (1 436 usages in
140 files) until its answer is complete; types a type error at the end of the file and polls
`problems.file.list` until it is there (**problems after an edit**: rust-analyzer's own analysis);
puts in a lifetime error only `cargo check` finds, saves, and polls until it is there (**check
after a save**), then fixes it, saves, and polls until it is gone; renames `zen_vfs::read_text`
(19 edits in 11 files of 3 crates) and back; runs `cargo test -p zen-vfs` with `waitMs`; reads
rust-analyzer's resident memory with `ps`. Other agents' IDEs ran on the same computer, so single
numbers vary by a few seconds; each row is two runs or more.

**Before and after** (seconds; "0.2.6" is the published RustRonin 0.2.6, "tuned" this change):

| | 0.2.6 cold | 0.2.6 warm | tuned cold | tuned warm |
|---|---|---|---|---|
| Ready | 17.3–25.0 | 8.2–18.9 | 20.3–23.8 | 8.7–9.4 |
| Go to Symbol right after | 10.0 (its limit: the start-up check was running) | 0.5–0.7 | 0.4–1.0 | 0.2 |
| Find Usages, 1 436 usages | 13.1–28.7 | 6.8–15.8 | 7.6–16.9 | 8.2–8.4 |
| The same again | 0.16 | 0.13 | 0.14–0.20 | 0.13 |
| Problems after an edit | never (see below) | never | 0.18–0.23 | 0.17 |
| Check after a save, until the problem shows | 6.8–7.2 | 7.3–7.5 | 2.7–3.5 | 0.5 |
| … and until the fix clears it | 6.4–7.0 | 5.1–5.2 | 0.5 | 0.4–0.5 |
| Rename across 3 crates, after the save | 7.2–7.9 (10 s, failed, in 2 of 5 runs) | 6.1–6.2 | 0.5 | 0.5 |
| Rename back | 0.8 | 0.8 | 0.7 | 0.7–0.8 |
| `cargo test -p zen-vfs` (11 tests) | 0.8–2.9 | 0.7–1.6 | 1.3 | 1.2 |
| rust-analyzer's memory once ready | 3.1–4.9 GB | 2.9–4.9 GB | 4.9 GB | 4.9 GB |

What made the difference:

- **Problems after an edit never came** in 0.2.6 once a lookup had run: rust-analyzer cancels a
  pulled-diagnostics request (`ServerCancelled`) while it reloads files, the IDE took the
  cancellation as an answer, and `problems.file.list` said the file was checked and clean. Now a
  cancelled request is asked again after 300 ms, and the file is "checking" until it is answered.
- **A save checks its package** (`check.workspace: false`): a save runs `cargo check -p <package>`
  instead of `--workspace`, and opening the project runs no check. The workspace-wide check had
  every lookup and rename after opening the project or saving a file wait for it.

Each setting, one at a time, warm unless named (same script; the IDE's own fix in all of them):

| Setting | Ready | Go to Symbol | Usages | Check after a save | Memory once ready | Kept |
|---|---|---|---|---|---|---|
| rust-analyzer's defaults with `cargo.targetDir` (the old defaults) | 8.5–11.0 (cold 18.1–48.9) | 0.4 (cold 10.0, or after a 49 s start) | 7.0–8.8 (cold 6.4–17.6) | 0.6–0.8 | 4.9 GB | |
| `check.workspace: false` | 8.3–8.4 (cold 17.3–22.5) | 0.2 (cold 0.2) | 6.8–6.9 (cold 7.8–8.5) | 0.5 | 4.9 GB | yes |
| `cachePriming.enable: false` | 3.5 | 6.0 (the index is built then) | 9.8 | — | 0.9 GB (4.0 GB after the usages) | no: the first lookup pays |
| `procMacro.enable: false` | 8.6 | 0.4 | 7.0 | — | 4.4 GB | no: derives unexpanded, false errors |
| `cargo.buildScripts.enable: false` | 8.9 | 0.2 | 6.9 | — | 4.9 GB | no: `build.rs` output unknown |
| `lru.capacity: 512` | 8.5–8.6 | 0.2 | 6.8–6.9 | — | 4.9 GB | no: nothing measurable |
| `checkOnSave: false` | 14.9 | 0.3 | 8.5 | no check at all | 4.4 GB | no |
| `files.exclude: [".claude/worktrees"]`, a one-package project holding a 17 MB copy of this repository there | 1.6 instead of 1.8 | | | | 524–572 MB instead of 546–585 MB | yes |

`cargo.targetDir: true` (checking into `target/rust-analyzer`) stays: `cargo test` and the user's
builds never wait for rust-analyzer's check. On a workspace whose root is not a package (this
one), rust-analyzer does not load `.claude/worktrees` anyway; the exclusion helps a one-package
repository with agents' worktrees inside. `files.exclude` takes folders, not patterns: a
`target-*` folder is left out by naming it in a project's `.zen/rust-analyzer.json`.

Go to Symbol asks rust-analyzer for every kind of symbol (`workspace.symbol.search.kind:
all_symbols`, not only types) and for up to 2000 of them (`workspace.symbol.search.limit`, 128 by
default). rust-analyzer matches the query as a subsequence of a name and stops at the limit, so with
128 a short name such as `route` came back as 128 names like `reformat_..._output_...` and never
reached `IdeApp::route`; the IDE's own matcher ranks the answers afterwards.

## Spelling (2026-10-22)

The spelling inspection ([ADR-0113](adr/0113-spelling-inspection-and-bundled-dictionary.md))
checks a file on a thread of its own; a keystroke costs the window's thread a copy of the text for
the next check and moving the marks already shown, and, for a highlighter that is not incremental
(a plug-in, Rust), a copy of the spans the window keeps anyway, so that a plug-in is never called off
the window's thread. `typing_in_a_large_file_does_not_wait_for_the_spelling_check`
in `go-garden` (`tests/spelling.rs`, `cargo test --release -p go-garden --test spelling -- --ignored
--nocapture`) types into a 10 000-line Go file with 446 typos and times `problems.file.list` after
each keystroke, as the editor asks for it every frame. Release build, Apple M-series:

| | p50 | p95 | max |
|---|---:|---:|---:|
| A keystroke's problems, spelling on | 0.28 ms | 0.36 ms | 0.51 ms |
| A keystroke's problems, spelling off | 0.00 ms | 0.00 ms | 0.04 ms |
| A frame without a keystroke, spelling on | 0.16 ms | | |
| The whole check of the file, in the background | 49.8 ms | | |

A debug build: 3.6 / 4.0 / 4.6 ms a keystroke and 233 ms for the whole check.

## MCP against plain tools (2026-10-24)

What a typical agent step costs through the IDE's MCP server ([ADR-0139](adr/0139-lean-mcp-answers-secrets-and-costs.md)) against the plain tools an agent has without it (its Read and Edit, `grep`/`rg`,
`cargo check`). Measured on this repository (about 2 000 Rust files) with a debug build of
RustRonin (`rustronin --headless --no-restore --mcp-port 63999 <repo>`, rust-analyzer 1.99.1,
warm), Apple M-series. Bytes are the answer's text, what the model reads; for the plain tools,
the output of the command (the Read tool's `cat -n` form for reads).

| Step | Through RustRonin | Plain tools |
|---|---|---|
| Read 40 lines of an 844-line file | `get_document_text` with `startLine`/`endLine`, `withLineNumbers`: 1 call, 1 519 B | Read with offset/limit: 1 call, 1 521 B |
| Edit, and learn the error it brought | `editor_text_replace` with `checkProblems`: 1 call, 118 B, 0.3 s, `newProblems: ["38:28 error no field \`resett\`…"]` | Edit (about 130 B) and `cargo check -p zen-workbench --message-format short`: 2 calls, 190 B, 9 s |
| The project's compile errors | `problems.project.list` `severity: error`, `compact`: 1 call, 143 B (one error), 25 B when clean | `cargo check --workspace` after an edit: 1 call, 24 s |
| A file's errors | `problems_file_list` `severity: error`, `compact`: 105 B (unfiltered 160 B) | — (the same `cargo check`) |
| Usages of `ReplaceParams` by name | `navigate_usages_find` `symbol`, `compact`: 1 call, 344 B, the 2 usages of this `ReplaceParams`; 952 B with `includeTextMatches` (the 6 places of the two other `ReplaceParams`) | `rg -nw ReplaceParams`: 1 call, 892 B, 9 lines of three different types mixed |
| 15 symbols named like `Params` | `navigate_symbol_goTo` `compact`, `limit: 15`: 971 B (objects: 3 089 B) | `rg` for declarations, 15 lines: 1 113 B |
| The open windows | `list_windows` with `--no-restore`: 387 B (one tab) | — (a windowed RustRonin with 26 restored tabs: 3.8 KB) |
| The tool list, once a session | `tools/list`: 65 968 B, 22.5 KB of it `set_settings`' schema; 42 148 B since ADR-0143 (2026-10-25: a short `set_settings`, lean command tool schemas) | — |

The five steps together: **5 calls and about 3.1 KB** through RustRonin, **6 calls, about 3.8 KB
and 9–24 s of compiling** with the plain tools, and the plain usages mixed three types of the
same name. The targets: 15 symbols under 1.5 KB (971 B), errors only under 2 KB
(143 B), an edit under 300 B (118 B with its new error), 40 of 800 lines under 3 KB (1 519 B).

The time an answer took is in its `_meta.elapsedMs`; `mcp.session.stats` summed this session: 18
calls, 1 268 B of arguments, 9 667 B of answers. The first Find Usages and the first symbol search
wait for rust-analyzer to load the workspace (17 s and 10 s here); afterwards a symbol search takes
0.2 s and Find Usages 4–5 s on this repository.

## UI tests without needless painting (2026-10-24)

`zen-ui`'s tests draw the IDE offscreen (`zen_ui::offscreen`): `Offscreen::frame` and `render` used
to rasterise every frame on the CPU, at 783 call sites, although most tests only run frames for
their effects and never read the pixels. A 60 s `sample` of the running test binary put about 38 %
of the CPU it used in `Offscreen::rasterize` (tessellation about 0.1 %); the rest was layout (the
IDE's own `logic` and `ui`), font hinting, allocation and Fluent messages. A
frame is laid out and tessellated at once but rasterised only when its picture is first read
(`Frame`, which dereferences to the `ColorImage`), and the tests' snapshot writers take frames, so a
picture is painted only when it is compared or saved (`ZEN_UI_SNAPSHOTS`).

The `zen_ui` unit tests (977, 4 ignored), debug build without debug info, run from
`crates/platform/ui` under `/usr/bin/time -l` on an Apple M-series Mac with other work running:

| | Wall | User CPU | Instructions | Peak memory |
|---|---:|---:|---:|---:|
| Before | 240 s | 1 155 s | 12.1 × 10¹² | 743 MB |
| After | 178 s | 553 s | 4.0 × 10¹² | 873 MB |

CPU time fell by 52 % and instructions by 67 %; memory rose because a frame keeps its meshes and
shares the textures until it is dropped. Single tests that saved nothing still painted every frame
through a snapshot helper taking a `ColorImage`: the indent test of the status bar went from 102 s
to 1 s, Zoom IDE from 82 s to 3 s, the read-only lock from 30 s to 0.9 s.

The wall time is now set by a few long tests that lay out many frames on one thread (the
navigation bar's placements 79 s, the Settings pages' search marks 65 s — those read pixels — the
command wheel 49 s, the code-folding page 40 s); `RUSTC_BOOTSTRAP=1 <test binary> -Z
unstable-options --report-time` lists each test's time.

The whole gate on this branch (`cargo xtask ci`, warm target, `CARGO_BUILD_JOBS=6`): the test step
356 s (4 to 7 minutes in the runs before, with the UI tests painting), the gate 365 s wall and
888 s user CPU; run again on the same committed tree it says the tree passed and ends in 1 s
(ADR-0141).

## Database results grid

`cargo test -p zen-ui --release grid_scroll_benchmark -- --ignored --nocapture` scrolls a grid of
1 000 000 rows × 300 columns (a synthetic source, `ide/data_grid/tests.rs`) for 600 frames,
up and down and sideways, jumping every 100 frames, and reports the time `data_grid::show` takes per
frame (input, the cells read and formatted for the view, the shapes; not tessellation).

2026-10-26, Apple M5 Pro, release build: p50 0.10 ms / p95 0.18 ms / max 0.85 ms. The row offset
is a whole number, so the last of a million rows shows exactly; only the visible cells (and 40 rows
and 6 columns around them) are read and formatted, once, and kept until the view leaves them or
rows arrive.

Splitting a 10 MB SQL script (`zen_db::sql`) takes under 0.3 s even in a debug build, and an edit
in its middle re-lexes a few lines (`ZEN_TIMING_CHECKS=1 cargo test -p zen-db sql::tests::splitting`).
