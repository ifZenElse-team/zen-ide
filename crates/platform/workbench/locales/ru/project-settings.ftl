# Настройки проекта из других IDE (project.settings.*, ADR-0051).
workbench-foreign-settings-refused = { $path } относится к настройкам проекта JetBrains, которые Zen IDE никогда не меняют автоматически; ничего не изменено. Если файл нужно изменить, отредактируйте его сами.

workbench-error-encoding = { $kind ->
    [invalid] { $file } не является корректным текстом в кодировке { $charset }. Файл открыт только для чтения и никогда не сохраняется, чтобы его байты остались прежними.
   *[charset] По настройкам проекта файл { $file } записан в кодировке { $charset }, которую Zen IDE пока не умеют редактировать. Файл открыт только для чтения и никогда не сохраняется, чтобы его байты остались прежними.
}
workbench-error-encoding-not-open = { $path } не открыт в редакторе: откройте его, чтобы сменить кодировку.
workbench-error-encoding-unknown = { $charset } — не кодировка, которую Zen IDE умеют читать и писать.
workbench-error-encoding-reload = Файл на диске — не текст в { $charset }, либо в нём есть несохранённые изменения: сначала сохраните его или выберите Convert.
workbench-error-encodings-write = Не удалось записать .idea/encodings.xml: { $error }
