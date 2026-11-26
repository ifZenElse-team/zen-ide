settings-keymap-label = Keymap:
settings-keymap-actions = Keymap actions
settings-keymap-duplicate = Duplicate…
settings-keymap-rename = Rename…
settings-keymap-delete = Delete
settings-keymap-import = Import…
settings-keymap-copy-name = { $name } copy
settings-keymap-broken = Cannot be used: { $error }
settings-keymap-rename-title = Rename Keymap
settings-keymap-delete-title = Delete Keymap
settings-keymap-delete-question = Delete the keymap “{ $name }”?
settings-keymap-import-title = Import Keymap
settings-keymap-files = JetBrains keymaps
settings-keymap-imported = Imported “{ $name }”: { $mapped ->
    [one] { $mapped } action
   *[other] { $mapped } actions
}; { $unmapped ->
    [one] { $unmapped } entry
   *[other] { $unmapped } entries
} not carried over
settings-keymap-import-failed = The keymap cannot be imported: { $error }
settings-keymap-foreign-parent = The keymap is based on “{ $parent }”; its own shortcuts were applied over the macOS keymap.
settings-keymap-no-parent = no parent
settings-keymap-reason-unknown-action = no such action here
settings-keymap-reason-second-keystroke = two keystrokes in a row are not supported
settings-keymap-reason-mouse = mouse shortcuts are not supported
settings-keymap-reason-unknown-key = the key is not supported
