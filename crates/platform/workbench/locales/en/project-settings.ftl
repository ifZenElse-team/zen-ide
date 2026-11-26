# Project settings of other IDEs (project.settings.*, ADR-0051).
workbench-foreign-settings-refused = { $path } belongs to the project's JetBrains settings, which Zen IDEs never change automatically; nothing was changed. Edit the file yourself if you mean to change it.

workbench-error-encoding = { $kind ->
    [invalid] { $file } is not valid { $charset } text. It is open read-only and is never saved, so that its bytes stay as they are.
   *[charset] The project's settings say { $file } is in { $charset }, which Zen IDEs cannot edit yet. It is open read-only and is never saved, so that its bytes stay as they are.
}
workbench-error-encoding-not-open = { $path } is not open in an editor; open it to change its encoding.
workbench-error-encoding-unknown = { $charset } is not an encoding Zen IDEs can read and write.
workbench-error-encoding-reload = The file on disk is not text in { $charset }, or it has unsaved changes: save it first, or choose Convert.
workbench-error-encodings-write = Cannot write .idea/encodings.xml: { $error }
