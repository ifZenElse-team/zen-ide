database-copy-error-empty-delimiter = Разделители столбцов и строк не могут быть пустыми.
database-copy-error-delimiter-quote = Разделитель не может содержать символ кавычки.
database-copy-cut-rows = Скопировано строк: { $rows } из { $total }; копия вмещает не больше 100 000 строк. Чтобы взять все строки, экспортируйте результат.
database-copy-cut-size = Скопировано строк: { $rows } из { $total }; копия вмещает не больше 64 МиБ текста. Чтобы взять все строки, экспортируйте результат.
database-copy-cut-limit = В ответе строк: { $rows } из { $total }; чтобы получить больше, увеличьте limit.
database-copy-cancelled = Копирование отменено; буфер обмена не изменился.
database-copy-running = Копируются строки: { $total }…
database-copy-error-unknown-job = Копирования { $job } нет.
database-paste-error-empty = Вставлять нечего.
database-paste-error-too-large = За раз можно вставить не больше { $most } ячеек.
database-paste-no-column = Здесь нет столбца { $column }.
database-paste-deleted-row = Строка помечена на удаление.
database-paste-error-invalid = { $count ->
    [one] { $count } значение не подходит своему столбцу; ничего не вставлено.
    [few] { $count } значения не подходят своим столбцам; ничего не вставлено.
   *[other] { $count } значений не подходят своим столбцам; ничего не вставлено.
}
