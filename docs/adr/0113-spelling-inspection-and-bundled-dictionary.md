# ADR-0113: The spelling inspection and its bundled English dictionary

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (clean-room, pure Rust),
  [ADR-0045](0045-themes.md) (theme keys), [ADR-0059](0059-go-mod-dependency-checks.md)
  (inspections), [ADR-0033](0033-ai-access-to-every-api.md) (every capability a command)

## Context

The requirement: the IDE underlines misspelt words of names, string literals and comments as **Typo**
problems (Settings → Editor → Inspections → Proofreading → Spelling, with Process code, Process
literals and Process comments), steps through them with F2 and ⇧F2 and never counts them as
errors. Zen IDE had no spell checker,
no dictionary and no Typo severity.

The dictionary must be clean-room and ship with the IDE, which works offline (AGENTS.md rules 2
and 10): no dictionary of another product, no Hunspell or SCOWL download, no C library. The
machine the work was done on has `/usr/share/dict/web2`: Webster's Second New International
Dictionary (1934),
234 936 words, whose "1934 copyright has lapsed, according to the supplier" (the list's
`README`); FreeBSD maintains it as "a list of current [American] English spellings" and ships it
with macOS. It is a clean, public-domain source, but it has no inflections (`files`, `parsed`,
`became` and `held` are missing), no modern or programming words (`email`, `config`, `byte`,
`dialog`) and some archaic spellings that hide common typos (`untill`).

Typing must never wait for the check: a 10 000-line file has tens of thousands of words.

## Decision

1. **A new platform crate `zen-spell` (L0)**, pure Rust, also for WebAssembly, knows words and
   nothing of files or languages:
   - **The dictionary** is `data/web2.z`, the web2 list lower-cased, sorted, front-coded (the
     length shared with the previous word, then the rest) and compressed with Deflate (`flate2`'s
     pure-Rust backend, already a workspace dependency): 518 486 bytes for 234 454 words, against
     2.4 MB of text. `cargo xtask spelling-dictionary [<web2 file>]` writes it from
     `/usr/share/dict/web2`; the same list gives the same bytes (`the_bundled_file_is_reproduced_from_the_word_list`,
     run with `--ignored`). The list used is macOS's copy of FreeBSD's `share/dict/web2`
     (2 493 885 bytes, SHA-256 `be41ad97963bf8dabedd5871d5d691596175269d540956b0f9965a885c2bbab9`);
     a machine without it takes the file from FreeBSD's source tree and passes its path. `data/extra.txt` adds the words the 1934 list lacks, written for Zen
     IDE: modern and programming words (`email`, `config`, `goroutine`, `unmarshal`...), common
     abbreviations of code (`ctx`, `cfg`, `src`...), irregular verb forms and contractions;
     `data/excluded.txt` leaves out archaic words that hide common typos (`untill`, `wether`), as
     FreeBSD did with `freend`.
   - **Decoded lazily** on the first check, on the spelling thread (`zen_spell::english`, a
     process-wide `OnceLock` every window shares): 3.4 MB in memory for 235 415 words (2.49 MB of
     sorted text and 0.94 MB of offsets), looked up by binary search; decoding holds the list as
     separate strings for a moment, about 10 MB more, freed at once.
   - **Rules instead of inflections:** a word is known when it is in the list, or is a regular
     inflection (`-s`, `-es`, `-ies`, `-ed`, `-ing` with a doubled consonant or a dropped `e`,
     `-er`, `-est`, `-ly`, `'s`, `n't`, `'ll`...) or derivation (`-ation`, `-ize`, `-ness`,
     `-ment`, `-able`...) of one, has a prefix (`un`, `re`, `pre`, `non`... before at least five
     letters), is two words of at least four letters written together (`filename`), or is a
     British spelling (`colour`, `centre`, `initialise`, `serialisable`, `licence`, `cancelled`).
     **American and British spellings are both known** (the review, 2026-10-22): the project's own
     documents are British, the code people write is often American, and marking either would
     underline correct words all over one of them. The rules accept
     some misspellings (`truely`, `begining`, `independant`), which is the price of a list without
     inflections; the tests list both what must be known and the common misspellings that must
     not be.
   - **Words** (`zen_spell::words`): names split at capitals, `_` and digits (`parseUrlHadnler`
     gives `parse`, `Url`, `Hadnler`; `HTTPServer` gives `HTTP`, `Server`). Skipped: words of one or
     two letters, words in capitals, words of one repeated letter (`aaaa`, `zzz`), URLs, e-mail
     addresses, file paths (a `/` next to a letter, a digit, `.` or `~`: `docs/guide.md`,
     `~/.zprofile`, `and/or`), dotted and qualified names (`fmt.Sprintf`, `main.go`,
     `space.ifzenelse.gogarden`, `egui::Context`), hexadecimal numbers and colours (`0x…`, hex
     digits with both digits and letters, `#fff`, `#ff00aacc`), escapes and format verbs in
     literals, letters written against digits outside code (`aarch64`, `GHSA-v778`; names in code
     still split at digits and check every piece), and words with letters outside ASCII, which an
     English dictionary cannot judge.
2. **The workbench's spelling inspection** (`zen_workbench::spelling`) checks every open text
   file on a thread of its own: the window asks for a file's typos as it draws, a new check is
   sent when the text, the settings, the highlighter or the language server's declarations
   change, and until it comes the last answer moves with the edits (`Document::char_edits_since`).
   The file's highlighter says what is a comment, a literal or code; **names are checked where
   they are declared**, so a library's names (`fmt.Sprintf`) are never
   marked: the names the language server's semantic tokens mark `definition` or `declaration`,
   or, without them, a name after a declaring keyword (`func`, `type`, `var`, `const`, `fn`, `let`,
   `struct`...; `mut` and `const` only when not after `&` or `*`, so `&mut Frame` and `*const u8`
   declare nothing) or before `:=`. Markdown, plain text and files no highlighter knows are
   checked word by word, code and link targets (a Markdown highlighter's strings) left out. Files
   over 4 MB are not checked. A window turns the inspection on (`Spelling::activate`); sessions
   without a window, such as tests, are left alone.

   **Threads** (the review, 2026-10-22). Only highlighters that lex incrementally (the built-in Go
   and Rust lexers, pure functions without locks) are called on the spelling thread. One that
   highlights the whole text, such as a WebAssembly plug-in, keeps its runtime behind a mutex the
   window also needs (highlighting, completion, typing); the check takes the spans the window
   already keeps for the current text (`Highlighters::highlighting`, a copy of a vector) instead of
   calling it, so the spelling thread never holds a plug-in's lock. A running check is not
   interrupted, but the thread takes the newest job of each file and drops the stale ones queued
   meanwhile, and an answer older than the one shown is ignored. The semantic tokens are read for
   a new job only, not every frame.
3. **Typo is a severity of its own** (`Severity::Typo`, `"typo"`), between a weak warning and
   information: problems with `source: "spelling"`, `code: "typo"` and the message
   "Typo: In word '<word>'". They are in `problems.file.list` (and so F2, ⇧F2, the editor's marks
   and counter and the Problems tool window's File tab), and **not** in `problems.project.list`,
   the Project Errors tab. Quick-fixes and dictionaries (later work) add `fixes` to these
   problems and more word lists to the check (`Dictionary::from_words` already builds one from a
   user's words); the found typos keep the word and where it is (`code`, `literal`, `comment`,
   `text`) for "Rename to…" and "Change to…".
4. **The theme's typo colour** is a new editor key, `diagnostic.typo` (Zen Dark `#5FA36A`, Zen
   Light `#4E9A5B`), imported from a JetBrains scheme's `TYPO` attribute.
5. **Settings and commands:** Settings → Editor → Inspections shows Proofreading → Spelling with
   its severity and Process code, Process literals and Process comments (`IdeSettings::spelling`,
   session keys `inspections.spelling.*`, `set_settings` `spelling`). `spelling.typos.list` gives a
   file's typos once the check of its current text is done (it waits up to `waitMs`), and
   `spelling.words.check` judges words.

## Consequences

- Misspellings are marked in every IDE and every language with a highlighter, offline, without a
  language server; with one, only the names it says are declared.
- The binary grows by about 0.53 MB (the 518 486-byte list and 9 KB of hand-written words, once:
  the debug `gogarden` and `rustronin` each hold one copy); WebAssembly plug-ins do not carry the
  dictionary, and `zen-spell` builds for `wasm32-unknown-unknown` (the gate's `wasm` step).
- The first check after start decodes the dictionary (about a quarter of a second in a debug
  build), off the window's thread; until then no typo is shown.
- A keystroke costs the window's thread a copy of the text for the next check and the move of the
  shown marks; the check of a 10 000-line file runs in the background (docs/performance.md).
- `problems.file.list` may lack typos for a moment after a change, as it may lack a language
  server's answer; `spelling.typos.list` waits for them.
- Which words count as short (one or two letters here) and which are skipped is our
  choice.

## Alternatives considered

- **SCOWL or Hunspell's `en_US` dictionary:** better coverage with affixes, but it would have to be
  downloaded and its licence reviewed; web2 is public domain and already on the machine.
- **Expanding the inflections into the bundled file:** several times larger; rules at lookup keep
  it at half a megabyte.
- **The check on the window's thread, line by line:** simpler and synchronous, but a file with
  many typos or a slow highlighter would cost each keystroke; the background thread costs a copy.
- **Every name occurrence, not only declarations:** flags every library name the user cannot
  change; names are checked where they are declared.

## Tuning on real code (the review, 2026-10-22)

A spell checker that underlines correct technical words everywhere is worse than none. The ignored
test `spelling_of_this_repository` (`apps/go-garden/tests/spelling.rs`) checks every Rust and
Markdown file of this repository (726 files) with the IDE's Go and Rust highlighters and the
Markdown plug-in: build the plug-in with `cargo build --target wasm32-unknown-unknown -p
zen-plugin-markdown`, then run `cargo test -p go-garden --test spelling spelling_of_this_repository
-- --ignored --nocapture`; `ZEN_SPELLING_TYPOS=<file>` writes every typo with its place.

| | Typos | Different words | Files with typos | Comments | Text | Names | Literals |
|---|---:|---:|---:|---:|---:|---:|---:|
| As implemented | 4 045 | 803 | 444 | 463 | 661 | 251 | 2 670 |
| After the review | 805 | 447 | 169 | 40 | 86 | 22 | 657 |

Of the first 4 045, 163 were misspellings the tests write on purpose; nearly all the rest were
correct words. The review changed the rules (the skips of file paths, dotted names, colours,
repeated letters and letters against digits; Markdown link targets; `&mut`) and added to
`data/extra.txt` only correct words in general use that the 1934 list lacks: programming terms
(`xtask`, `egui`, `jsonrpc`, `heredoc`, `tbody`, `sysroot`), modern English (`trackpad`,
`caffeinate`, `mistyped`, `dogfood`), month abbreviations, the names of the Zen IDE products and
Japanese words in English use (`ronin`, `tatami`, `sensei`). A word is never added only to quiet
one project: names of a project's own (variables such as `doy`, tools such as `taplo`) are for
user and project dictionaries. What is left: 154 misspellings of the tests, the
data tables of the HTML and CSS plug-ins and of the suffix rules (attribute and colour names,
`ization`), fuzzy-search queries in tests (`ite`, `tem`) and a few real typos of the repository
(`manieth`); comments, text and declared names keep 148 marks in 726 files.
