# Zen Workbench: сравнение схем и данных в рабочей области Database.

database-compare-title = Сравнение
database-compare-error-unknown = Сравнения { $id } нет.
database-compare-error-not-relational = Сравнивать можно только SQL-базы данных.
database-compare-error-not-connected = { $connection } не подключено, поэтому его структуру прочитать не удалось.
database-compare-error-no-schema = В структуре { $connection } нет схемы { $schema }. Сначала обновите её в навигаторе баз данных.
database-compare-error-no-file = Файла снимка { $path } нет.
database-compare-error-snapshot = Не удалось прочитать файл снимка { $path }: { $error }
database-compare-error-snapshot-version = Файл снимка записан другой версией (формат { $version }).
database-compare-error-not-finished = Сравнение ещё не закончилось.
database-compare-error-no-table = Укажите таблицу для сравнения.
database-compare-error-no-key = В структуре таблицы нет первичного ключа: укажите ключевые столбцы.
database-compare-error-no-columns = Столбцы таблицы ещё не известны: обновите её структуру или укажите столбцы.
database-compare-error-not-read = Схема { $schema } ещё не прочитана: сначала обновите её.
database-compare-note-not-dropped = { $object } есть только в цели; не удаляется.
database-compare-note-not-dropped-part = { $part } в { $object } есть только в цели; не удаляется.
database-compare-note-cannot-alter = { $object } { $part }: эта база данных не умеет менять это на месте; пересоздайте вручную.
database-compare-note-by-hand = { $object } { $part }: измените вручную.
database-compare-note-no-ddl = { $object }: DDL построить не удалось (подробности не прочитаны).
database-compare-note-rows-not-deleted = { $count ->
    [one] { $count } строка, которая есть только в цели, не удаляется.
    [few] { $count } строки, которые есть только в цели, не удаляются.
   *[many] { $count } строк, которые есть только в цели, не удаляются.
}
database-compare-note-rows-truncated = Сохранены только первые { $kept } различающихся строк; после этого скрипта сравните снова.
