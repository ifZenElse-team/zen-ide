# Zen Workbench: импорт CSV в таблицу рабочей области «База данных».

database-error-import-delimiter = Разделитель, кавычка и символ экранирования должны различаться, и ни один из них не может быть переводом строки.
database-error-import-no-file = { $path } — не файл.
database-error-import-read = Не удалось прочитать { $path }: { $error }
database-error-import-not-table = { $path } — не таблица: укажите [база, схема, "tables", имя].
database-error-import-unsupported = Драйвер этого подключения не работает с SQL, поэтому импортировать в него CSV нельзя.
database-error-import-table-exists = { $table } уже существует; импортируйте в неё, а не создавайте заново.
database-error-import-no-columns = Ни один столбец файла не попадает в { $table }: сопоставьте хотя бы один.
database-error-import-mapping = Сопоставление столбца { $column } неверно: каждый столбец — один раз и из поля, которое есть в файле.
database-error-unknown-import = Импорта { $id } нет.
database-import-title = Импорт из CSV
database-import-done = { $rows ->
    [one] В { $table } импортирована { $rows } строка.
    [few] В { $table } импортировано { $rows } строки.
   *[other] В { $table } импортировано строк: { $rows }.
}
database-import-done-skipped = В { $table } импортировано строк: { $rows }; пропущено: { $skipped }:
    { $errors }
database-import-failed = Импорт в { $table } не удался: { $reason }. Строк зафиксировано до остановки: { $committed }.
database-import-cancelled = Импорт в { $table } отменён. Строк зафиксировано до остановки: { $committed }.
database-import-row = Строка { $row } (строка файла { $line }): { $reason }
database-import-row-columns = число полей не совпадает с заголовком
database-import-row-number = { $column } — не число
database-import-row-boolean = { $column } — не true и не false
database-import-row-date = { $column } не подходит под формат даты и времени
database-error-import-sql-type = Тип «{ $sql_type }» столбца { $column } — не тип столбца: используйте буквы, цифры, пробелы, _ . , и парные скобки.
