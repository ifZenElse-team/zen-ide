# Zen UI: вкладка «Поиск в базе данных» рабочего пространства Database.

ide-dbsearch-open = Поиск в базе данных
ide-dbsearch-text = Что найти
ide-dbsearch-no-connection = Нет подключения
ide-dbsearch-no-schema = Схема
ide-dbsearch-schemas-unknown = Подключитесь, чтобы увидеть схемы
ide-dbsearch-find = Найти
ide-dbsearch-stop = Остановить
ide-dbsearch-data = Данные
ide-dbsearch-names = Имена
ide-dbsearch-match-case = Учитывать регистр
ide-dbsearch-views = Представления
ide-dbsearch-columns-of = Столбцы:
ide-dbsearch-kind-text = Текст
ide-dbsearch-kind-number = Числа
ide-dbsearch-kind-temporal = Даты и время
ide-dbsearch-kind-other = Прочие
ide-dbsearch-only-columns = Только столбцы:
ide-dbsearch-only-columns-hint = email, name
ide-dbsearch-rows-per-table = Строк на таблицу:
ide-dbsearch-seconds-per-table = Секунд на таблицу:
ide-dbsearch-not-sql = Это подключение не использует SQL, поэтому искать в нём нельзя.
ide-dbsearch-intro = Введите текст и нажмите Enter: таблицы схемы просматриваются на сервере по одной, каждая до своего предела строк и срока.
ide-dbsearch-starting = Запуск…
ide-dbsearch-listing = Получаем список таблиц { $schema }…
ide-dbsearch-running = Ищем в { $table }… таблиц: { $ended } из { $total }, найдено в { $found }
ide-dbsearch-failed = Поиск не удался: { $reason }
ide-dbsearch-cancelled = Остановлено после { $ended } из { $total } таблиц, найдено в { $found }
ide-dbsearch-nothing = Ничего не найдено в { $total } таблицах ({ $seconds } с)
ide-dbsearch-done = { $rows ->
    [one] { $rows } строка
    [few] { $rows } строки
   *[other] { $rows } строк
} в { $found } из { $total } таблиц ({ $seconds } с)
ide-dbsearch-table = таблица
ide-dbsearch-view = представление
ide-dbsearch-materialized-view = материализованное представление
ide-dbsearch-rows = { $rows ->
    [one] { $rows } строка
    [few] { $rows } строки
   *[other] { $rows } строк
}
ide-dbsearch-rows-more = { $rows }+ строк
ide-dbsearch-timeout = остановлено через { $seconds } с
ide-dbsearch-table-failed = ошибка: { $reason }
ide-dbsearch-name-matches = совпадает имя
ide-dbsearch-named-columns = столбцы с таким именем: { $columns }
ide-dbsearch-open-rows = Открыть строки
ide-dbsearch-open-column = Только строки, где текст есть в { $column }
ide-dbsearch-menu = Поиск в базе данных…
