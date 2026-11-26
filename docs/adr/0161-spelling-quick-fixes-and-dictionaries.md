# ADR-0161: Spelling quick-fixes and the user's dictionaries

- **Status:** Proposed
- **Date:** 2026-10-22
- **Amends:** [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md) (decision 4: the project
  dictionary in `.idea` is written, on the user's request)
- **Relates to:** [ADR-0113](0113-spelling-inspection-and-bundled-dictionary.md) (the spelling
  inspection), [ADR-0058](0058-code-actions.md) and [ADR-0059](0059-go-mod-dependency-checks.md)
  (⌥⏎ and quick-fixes), [ADR-0044](0044-how-a-refactoring-changes-files.md) (rename),
  [ADR-0033](0033-ai-access-to-every-api.md) (every capability a command),
  [ADR-0002](0002-open-source-clean-room-pure-rust.md) (clean-room)

## Context

ADR-0113 marks misspelt words as Typo problems against a bundled English dictionary. The
requirement (the UI specification): ⌥⏎ on a typo in a
comment or literal shows a greyed **Fix typo** over the suggestions
as plain items, then **Save '<word>' to dictionary**; on a misspelt declared name it offers **Typo:
Rename to '<best>'** (one item) and Save. With **Use single dictionary for saving words** off (the
default) Save asks in a **Select Dictionary** popup: application-level, project-level. Settings →
Editor → Natural Languages → Spelling keeps that check box with a project-level / application-level
combo, **Custom dictionaries** (plain word lists and Hunspell `.dic` with `.aff`, listed under the
two built-in ones), **Accepted words** and a link to the Spelling inspection. The project-level
dictionary is `.idea/dictionaries/project.xml`, shared with the team and with the JetBrains IDEs;
the application-level one belongs to the IDE. A saved word is taken out again with ⌘Z.

ADR-0051 says Zen IDEs never write `.idea` files (ADR-0123 lets them write the excluded folders of
module files). A project dictionary that JetBrains IDEs and Zen
IDEs share has to be written by both. No Hunspell dictionary is bundled or downloaded (offline,
clean-room; no C library): the user brings their own.

## Decision

1. **Three kinds of user dictionary**, read by `zen_spell` (L0, pure functions of text) and kept by
   `zen_workbench::spelling` (a window's `Spelling` state), checked together with the bundled
   English dictionary (`zen_spell::typos_with`, `Dictionary::knows`, so `goroutine` saved also
   accepts `goroutines`):
   - **Application-level:** a plain word list, one lower-case word a line, at
     `<IDE data folder>/dictionaries/application.dic` (`Spelling::set_application_dictionary`, set
     by the window), so each IDE (GoGarden, RustRonin…) has its own.
   - **Project-level:** every `.idea/dictionaries/*.xml` of the project, the JetBrains format
     (`<component name="ProjectDictionaryState">`, its `<dictionary name="…">` elements, their
     `<words>` of `<w>` elements; `zen_spell::jetbrains`). Words are saved to
     `.idea/dictionaries/project.xml` (`<dictionary name="project">`, created on first save) and
     removed from every file that has them. **The file is edited in place**: only the `<words>`
     element of a dictionary is rewritten, a new word goes in its sorted place, existing words keep
     their order, and every other byte (declaration, comments, other elements and attributes,
     indentation, tabs) stays as written; removing a dictionary's last word removes its `<words>`.
     A file that cannot be parsed is never written. `.idea`, `.idea/dictionaries` and `project.xml`
     must not be symbolic links (refused, read nothing), and files over 4 MB are not read.
   - **Custom dictionaries** (setting `spelling.customDictionaries`, paths, relative ones against
     the project): a `.dic` with an `.aff` beside it is Hunspell, anything else a plain word list
     (blank lines and lines starting with `#` left out). Custom dictionaries are only read, never
     written; files over 64 MB are refused.

   Files are read again when their size or modification time changes, checked at most once a
   second (at once after the IDE's own save or removal), so a word saved by another program or in
   another window counts within a second; a change raises a generation that is part of a file's
   check key, so open files are checked again.
2. **The Hunspell subset** (`zen_spell::hunspell::words`), written from the hunspell(5) format
   description, expands a dictionary into every word it spells: `SET` (UTF-8, else ISO 8859-1),
   `FLAG` (single characters, `long`, `num`, `UTF-8`), `AF` flag aliases, `PFX`/`SFX` tables with
   strip, affix and condition (`.`, a letter, `[abc]`, `[^abc]`), the cross product of a prefix and a
   suffix when both allow it, `NEEDAFFIX` (a stem that is not a word on its own) and `FORBIDDENWORD`.
   Not read: compounding (`COMPOUND*`), affixes on affixes (continuation classes), `CIRCUMFIX`,
   `ICONV`/`OCONV`, `IGNORE`, the suggestion tables (`TRY`, `REP`, `MAP`) and morphological fields.
   Like the bundled dictionary, only words of English letters and apostrophes are kept (ADR-0113:
   words with other letters are not judged); at most four million forms.
3. **Suggestions by edit distance** (`zen_spell::suggestions`): every word of the dictionaries at
   most two edits away (insert, remove, replace a letter, swap two neighbours; the optimal string
   alignment distance, bounded), plus the one-edit candidates the rules know (`recieved` →
   `received`, an inflection the list lacks), ranked by distance, then swaps and doubled letters
   first (`hadnler` → `handler`, `adress` → `address`), the same first letter, the closest length;
   written in the typo's case (`Hadnler` → `Handler`). At most ten in the lists.
4. **The quick-fixes.** A typo problem carries **Save '<word>' to dictionary**, of kind `action`:
   command `spelling.word.save` while Use single dictionary is on, else the window action
   `view.spelling.saveTo`, the Select Dictionary popup (application-level first, then
   project-level). The suggestions cost too much to compute for every typo of a file, so
   `problems.fixes.list` (⌥⏎, ⌥⇧⏎) adds them for the typo at the place asked, before Save:
   - in a comment, a literal or text: a fix of the new kind `heading` (`zen_project::inspect::FixKind::Heading`,
     a greyed caption never picked), **Fix typo**, then one `action` fix per suggestion, titled
     with the word, running `spelling.typo.change` (one undoable edit);
   - in a declared name: **Typo: Rename to '<name>'**, the name with the misspelt part replaced by
     the best suggestion (`parseUrlHadnler` → `parseUrlHandler`), running `refactor.rename.apply`
     with `newName` (the language server's rename of ADR-0044).

   `problems.file.list` and the hover popup keep only Save. A typo's fixes are offered on the word
   only: ⌥⏎ elsewhere on its line keeps offering the line's other quick-fixes or the language
   server's context actions (`problems.fixes.list` leaves typos out of its line fallback). Agents
   run the `action` fixes with `run_action` or the commands directly.

5. **Undo.** `spelling.word.save` with the `path` of the file the word was found in remembers the
   save with that file's version. `editor.edit.undo` (⌘Z) first takes the last such word out again
   when the active file is that file and has not changed since, so ⌘Z right after Save to dictionary
   undoes the save and leaves the text alone; after an edit, ⌘Z undoes the edit and the word stays.
   Undo is not redone (⇧⌘Z does not save the word again).
6. **Commands** (ADR-0033), besides ADR-0113's `spelling.typos.list` and `spelling.words.check`
   (which now also knows the user's words):

   | Command | Parameters | Result |
   |---|---|---|
   | `spelling.typo.get` | `{ path?, offset? }` | the typo there, `suggestions`, and for a name `name { start, end, name, renames }` |
   | `spelling.typo.change` | `{ path?, offset?, to }` | `{ path, word, start, to }` |
   | `spelling.suggestions.list` | `{ word, limit? }` | `[word]` |
   | `spelling.word.save` | `{ word, level?, path? }` | `{ word, level, file, added }` |
   | `spelling.word.remove` | `{ word, level? }` | `[file]` |
   | `spelling.dictionaries.get` | `{}` | `{ useSingleDictionary, savingDictionary, application, project, custom }` |

   `level` is `application` or `project`; without it, `spelling.word.save` saves to the saving
   dictionary and `spelling.word.remove` takes the word out of both.
7. **Settings** (`IdeSettings::spelling`, `set_settings` `spelling`, session keys):
   `customDictionaries` (`spelling.customDictionaries`), `useSingleDictionary`
   (`spelling.useSingleDictionary`, off by default) and `savingDictionary`
   (`spelling.savingDictionary`, `project` by default); a command that
   names no level while the setting is off saves to application-level, the first of Select
   Dictionary. **Settings → Editor → Natural Languages** lists English (USA) and links to the
   Proofreading inspections; **→ Spelling** has, top to bottom, Use single dictionary for saving
   words with project-level / application-level (disabled while off), Custom dictionaries with
   Add, Remove and Edit (opens the file in the editor and closes the dialog, applying it) under
   the two built-in rows Application-level dictionary and Project-level dictionary, Accepted
   words with Add and Remove (the words of both user dictionaries; added words go to the saving
   dictionary, application-level while the setting is off, on OK or Apply as `spelling.word.save`,
   removed ones as `spelling.word.remove`) and Configure 'Spelling' inspection… (opens Editor →
   Inspections). No Grammar and Style page: Zen IDEs have no grammar checker.

## Consequences

- Words saved in a Zen IDE are seen by a JetBrains IDE that opens the project and the other way
  round, through the same `.idea/dictionaries/` files; the IDEs' own levels stay apart.
- `.idea/dictionaries/project.xml` is the one `.idea` file a Zen IDE writes, and only when the user
  saves or removes a word at project level; ADR-0051's guards (`guard::refuses_foreign_settings`)
  still keep refactorings, Replace in Files and other automatic writers out of `.idea`.
- Suggestions scan the 235 000 words for each typo shown in a list: tens of milliseconds in a release
  build, a few hundred in a debug one, on the window's thread when a list opens, never while typing.
- A large Hunspell dictionary is expanded in memory once per change of its files (en_US makes a few
  hundred thousand forms); dictionaries in other languages add only their ASCII words.
- The labels, the defaults, Select Dictionary, the file name `project.xml` and the Spelling page's
  layout are in the UI specification; how many suggestions Fix typo
  lists is still open.
- `FixKind` gains `heading`; clients that list fixes show it greyed and never pick it.

## Alternatives considered

- **Keep the project dictionary in `.zen/`:** never touches `.idea`, but the words would not be shared
  with the JetBrains IDEs, which the requirement asks for.
- **Rewrite the XML file from the parsed words:** simpler, but it would reformat the file and drop
  what another program or a person put there; editing the `<words>` element in place keeps diffs to
  the changed lines.
- **The application-level dictionary in `settings.json`:** one file fewer, but every save would
  rewrite the IDE's settings, and the word list would be mixed with them.
- **A full Hunspell implementation or a crate for it:** compounding and continuation classes matter
  for languages the check does not judge yet; the subset is enough for English and plain lists and
  needs no new dependency.
- **Typo: Change to… and Typo: Rename to… opening their own lists:** a step more; ⌥⏎ lists
  the suggestions itself and renames to the best name in one step, and Rename (⇧F6) stays for any
  other name.
- **Suggestions in every typo problem:** simpler for clients, but a file with many typos would pay
  for every list on each check; they are computed only for the typo ⌥⏎ is asked about.
