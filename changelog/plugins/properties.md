# Properties plug-in changelog

Every release of the Properties plug-in (`plugins/properties`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-04

### Added

- The first release, for `.properties` files; JvmJizo comes with it, and every other IDE offers it
  in a banner when a `.properties` file opens. It is also in the Marketplace.
- Highlighting of keys, separators, values, escapes and comments, as `java.util.Properties` reads
  them (`=`, `:` or white space between key and value, lines continued with `\`, `#` and `!`
  comments, `\uXXXX` escapes).
- Folding of continued values and comment blocks, and the keys in the Structure view.
- Duplicate keys reported as warnings, malformed `\u` escapes as errors.
- The value without its escapes shown as an inlay after it (`= Привет`).
- Keys completed, and found with Go to Declaration, in the string literals of `getString`,
  `getObject`, `getStringArray`, `containsKey` and `getMessage` calls in Java, Kotlin, Groovy and
  Scala, and bundle names in `ResourceBundle.getBundle`.
- Resource bundles (`messages.properties`, `messages_ru.properties`...) grouped in the Project tool
  window and edited in the Resource Bundle editor.
