# Zen Workbench: перенос данных между базами в рабочей области «База данных».

database-error-transfer-unsupported = { $name } не работает с SQL, поэтому копировать таблицы из него и в него нельзя.
database-error-transfer-no-table = { $path } — не таблица в структуре источника; обновите её и попробуйте снова.
database-error-transfer-schema = { $path } — не схема: укажите [база, схема].
database-error-transfer-no-tables = Выберите хотя бы одну таблицу для копирования.
database-error-transfer-no-columns = Ни один столбец не попадает в { $table }: сопоставьте хотя бы один.
database-error-transfer-mapping = Сопоставление столбца { $column } неверно: каждый столбец источника — один раз и каждый в свой столбец цели.
database-error-transfer-loading = Столбцы таблиц ещё читаются; попробуйте через мгновение.
database-error-unknown-transfer = Переноса { $id } нет.
database-transfer-title = Перенос данных
database-transfer-done = { $rows ->
    [one] Скопирована { $rows } строка
    [few] Скопированы { $rows } строки
   *[many] Скопировано { $rows } строк
} в { $tables ->
    [one] { $tables } таблицу
    [few] { $tables } таблицы
   *[many] { $tables } таблиц
}.
database-transfer-done-skipped = { $rows ->
    [one] Скопирована { $rows } строка
    [few] Скопированы { $rows } строки
   *[many] Скопировано { $rows } строк
} в { $tables ->
    [one] { $tables } таблицу
    [few] { $tables } таблицы
   *[many] { $tables } таблиц
}; пропущено: { $skipped }.
database-transfer-failed = Копирование { $table } не удалось: { $reason }. Строк зафиксировано до остановки: { $committed }.
database-transfer-cancelled = Перенос отменён после таблиц: { $done } из { $tables }. Строк зафиксировано до остановки: { $committed }.
database-error-transfer-same-table = Таблица { $table } была бы скопирована сама в себя: выберите другую целевую схему или таблицу.
database-error-transfer-too-many = За один перенос можно скопировать не больше { $most } таблиц; выберите меньше.
