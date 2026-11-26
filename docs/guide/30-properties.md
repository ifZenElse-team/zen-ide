# Properties files and resource bundles

The Properties plug-in edits `.properties` files as Java reads them (`java.util.Properties`), and
the Resource Bundle editor edits the translations of a message bundle side by side. The plug-in
works in every Zen IDE: JvmJizo comes with it, and the other IDEs offer it in a banner when you open
a `.properties` file ([Plug-ins](10-plugins.md)).

## Editing a .properties file

- **Highlighting** of keys, the separator (`=`, `:` or white space), values, escapes (`\t`, `\n`,
  `\uXXXX`, `\=`...) and comments (`#` and `!`).
- **Folding** of a value continued over several lines (a line that ends with `\`) and of a block of
  comment lines.
- **The Structure view** (**⌘7**, **⌘F12** for the popup) lists the keys in their order.
- **Problems**: a key given twice is a warning on each occurrence, and a `\u` not followed by four
  hexadecimal digits is an error.
- **The value as Java reads it**: after a value with escapes or continued lines, a grey inlay shows
  it unescaped, `= Привет` after `Привет`. The inlays follow
  [Settings → Editor → Inlay Hints](09-settings.md#editor--inlay-hints).
- In a file of a bundle, completion (**⌃Space**) at the start of a line offers the keys the bundle's
  other files have and this one lacks.

Since Java 9, resource bundles in `.properties` files are read as UTF-8, so you can write
translations as they are. `Properties.load(InputStream)` still reads ISO-8859-1: for files read
that way, write characters beyond Latin-1 as `\uXXXX` escapes, or save the file as ISO-8859-1 with an
`.editorconfig` that sets `charset = latin1` for it.

## Keys in Java and Kotlin code

In `.java`, `.kt`, `.kts`, `.groovy` and `.scala` files, inside the string literal that is the first
argument of `getString`, `getObject`, `getStringArray`, `containsKey` or `getMessage`:

- **Completion** offers the keys of the project's `.properties` files; when the file names bundles
  with `ResourceBundle.getBundle("…")`, only the keys of those bundles.
- **Go to Declaration** (**⌘B**, or ⌘-click) opens the key in its bundle's default file.

Inside `getBundle("…")`, completion offers the bundles by name (`i18n.messages` for
`src/main/resources/i18n/messages.properties`), and Go to Declaration opens the bundle's default
file.

## Resource bundles

The files `messages.properties`, `messages_ru.properties`, `messages_de_DE.properties`... of one
folder are one bundle, `messages`: the default file and one file per locale. In the Project tool
window they are grouped under a node **Resource Bundle 'messages'**; expand it to reach the files,
and double-click it (or select it and press **⏎**) to open the Resource Bundle editor.

The editor is a tab with a table: a row per key, a column per file (**Default**, then the locales).
A value that is missing or empty is marked **missing** and its key is coloured; **Missing
translations only** shows just those keys, and the filter field finds keys and values.

- Double-click a cell to edit the value; **⏎** writes it, escaped, into that file, **⎋** cancels.
- **+** (**New Property**) adds a key to every file of the bundle: the value you then type into the
  Default column, empty (missing) in the others.
- The pencil (**Rename Property**, **⇧F6**) renames the selected key in every file.
- The bin (**Remove Property**, **⌦**) removes the selected key from every file.
- **Undo** (**⌘Z** in the editor) takes back the last of these changes in all the files at once.

Files open in the editor change in their tabs and are saved as usual; the others are written at once.

## For agents

`properties.bundle.list` lists the bundles of the project or of a folder, `properties.bundle.get`
gives a bundle's table, and `properties.bundle.setValue`, `addKey`, `renameKey`, `removeKey` and
`undo` change it. `view.resourceBundle.open` opens the editor, and `view.resourceBundle.select`,
`filter`, `addKey` and `renameKey` drive it. Completion and Go to Declaration are the editor's own
commands (`editor.completion.start`, `navigate.declaration.goTo`).
