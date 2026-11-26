# Zen Workbench: рабочая область «База данных» и её консоли.

module-database-name = Базы данных
module-database-description = Рабочая область «База данных»: SQL-консоли в папке данных IDE, готовые к подключениям; навигатор базы и редакторы данных появятся следом.
database-workspace-title = База данных
database-overview-title = Обзор
database-error-no-data-folder = У IDE нет папки данных, где хранить консоли.
database-error-connection-name = «{ $name }» не подходит для папки консолей подключения: укажите простое имя без косых черт, двоеточий и точки в начале.
database-error-no-connection = Нет подключения, в котором открыть консоль: сначала добавьте его кнопкой «Новое подключение…».
database-error-no-connection-agent = Нет подключения, в котором открыть консоль: сначала добавьте его через `db.dataSource.add`.
database-error-files = Не удалось прочитать или записать файлы подключений к базам данных: { $error }
database-error-unknown-connection = Подключения к базе данных «{ $id }» нет.
database-error-not-external-driver = Внешнего драйвера баз данных «{ $id }» нет.
database-error-unknown-driver = Драйвера баз данных «{ $driver }» нет.
database-error-bad-url = Не удалось разобрать URI подключения: пишите его как scheme://user@host:port/database.
database-error-no-name = Нужно имя.
database-error-color = «{ $color }» — не цвет темы; используйте один из: { $colors }.
database-error-unknown-type = Типа подключения «{ $id }» нет.
database-error-type-exists = Тип подключения «{ $id }» уже есть.
database-error-type-built-in = { $name } — встроенный тип подключения, его нельзя удалить.
database-error-type-used = Тип { $name } используют подключения { $connections }; сначала задайте им другой тип.
database-error-not-replayed = Подключение потеряно во время выполнения запроса. Подключение снова открыто, но запрос не выполнялся повторно: проверьте, применился ли он. { $detail }
database-error-rolled-back = В транзакции была ошибка, поэтому сервер откатил её вместо фиксации: ни одно её изменение не сохранено.
database-error-cancel-unconfirmed = Сервер не подтвердил отмену за секунду; сеанс закрыт и откроется при следующем использовании.
database-error-no-server-cancel = Эта база данных не умеет отменять запросы на сервере; сеанс закрыт и откроется при следующем использовании.
database-error-connection = Не удалось подключиться к базе данных: { $detail }
database-error-authentication = Сервер не принял имя пользователя или пароль: { $detail }
database-error-tls = Ошибка TLS: { $detail }
database-error-password-needed = Для этого подключения нужен пароль: введите его и отметьте «Сохранить пароль», чтобы не вводить снова.
database-error-timeout = Время ожидания истекло.
database-error-no-columns = Запрос не вернул столбцов для показа.
database-error-cancelled = Отменено.
database-error-server = { $detail } [{ $code }]
database-error-not-connected = Подключение не установлено; сначала подключитесь.
database-error-open-transactions = Открыты транзакции в { $contexts }: зафиксируйте или откатите их либо отключитесь с transactions = commit или rollback.
database-error-invalid = Настройки подключения не подходят: { $detail }
database-error-not-repeatable = Чтобы прочитать больше строк, запрос нужно выполнить снова, а он может изменить данные; выполните его снова в консоли.
database-error-unsupported = Здесь это не поддерживается: { $detail }
database-error-cancel-what = Укажите операцию или подключение для отмены.
database-error-unknown-operation = Операции с базой данных { $id } нет; хранятся последние 200.
database-notice-idle-closed = Подключение { $name } простаивало и было закрыто; оно откроется при следующем использовании.
database-notice-secret-failed = Не удалось сохранить или удалить секрет подключения: { $detail }
database-notice-slept = { $count ->
    [one] Компьютер уходил в сон, поэтому { $count } подключение к базе данных закрыто; оно откроется при следующем использовании.
    [few] Компьютер уходил в сон, поэтому { $count } подключения к базам данных закрыты; они откроются при следующем использовании.
   *[many] Компьютер уходил в сон, поэтому { $count } подключений к базам данных закрыто; они откроются при следующем использовании.
}
database-notice-transaction-ended = Простаивавшая транзакция { $name } ({ $context }) откачена.
database-error-secret-empty = Секрет не может быть пустым.
database-error-secret-id = «{ $connection }» не подходит как подключение: укажите его идентификатор без косых черт.
database-error-secret-denied = Доступ к элементу связки ключей запрещён. Разрешите его в запросе связки ключей и повторите.
database-error-secret-timeout = Связка ключей не ответила вовремя, поэтому сохранённый пароль не прочитан. Если macOS показывает запрос связки ключей (разблокируйте экран, чтобы его увидеть), разрешите в нём доступ и подключитесь снова.
database-error-secret-unsupported = В этой системе нет хранилища секретов, которым может пользоваться IDE: пока секреты хранятся только в связке ключей macOS.
database-error-secret-failed = Связка ключей не смогла сохранить секрет: { $reason }
database-error-secret-no-store = У этого окна нет хранилища секретов.
database-cause-dns = Вероятная причина: имя хоста неизвестно; проверьте его написание и сеть или VPN.
database-cause-refused = Вероятная причина: на этом хосте и порту никто не слушает; проверьте, что сервер запущен и порт верный.
database-cause-tls = Вероятная причина: TLS; проверьте режим SSL и сертификаты, а также есть ли TLS на сервере.
database-cause-authentication = Вероятная причина: имя пользователя или пароль; проверьте их и правила доступа сервера.
database-cause-timeout = Вероятная причина: сервер не ответил вовремя; проверьте хост, файрвол или SSH-туннель.
database-error-no-console = Нет активной консоли или файла .sql: откройте его или укажите в `path`.
database-error-no-console-connection = У консоли нет подключения: выберите его в списке подключений на панели инструментов консоли.
database-error-no-console-connection-agent = У консоли нет подключения: выберите его через `db.console.chooseConnection`.
database-error-no-statement = Здесь нечего выполнять.
database-error-read-console = Не удаётся прочитать { $path }: { $error }
database-error-unknown-result = Нет результата { $id }; закрытые результаты удаляются.
database-error-unknown-query = Нет запроса { $id }; хранятся последние 100.
database-error-confirm-fetch-all = Прочитано строк: { $rows }. Чтение всех строк может занять много времени и памяти; подтвердите, чтобы продолжить.
database-result-title = Результат { $number }
database-console-hint-time = { $ms } мс
database-console-hint-rows = { $time } · { $rows ->
    [one] { $rows } строка
    [few] { $rows } строки
   *[other] { $rows } строк
}
database-console-hint-updated = { $time } · изменено { $rows ->
    [one] { $rows } строка
    [few] { $rows } строки
   *[other] { $rows } строк
}
database-console-hint-failed = ошибка
database-folder-tables = Таблицы
database-folder-foreign-tables = Внешние таблицы
database-folder-views = Представления
database-folder-materialized-views = Материализованные представления
database-folder-functions = Функции
database-folder-procedures = Процедуры
database-folder-sequences = Последовательности
database-folder-types = Типы
database-folder-events = События
database-folder-packages = Пакеты
database-folder-columns = Столбцы
database-folder-constraints = Ограничения
database-folder-foreign-keys = Внешние ключи
database-folder-indexes = Индексы
database-folder-triggers = Триггеры
database-folder-references = Ссылки
database-folder-policies = Политики
database-folder-partitions = Секции
database-folder-dictionaries = Словари
database-navigator-loading = Загрузка…
database-navigator-not-read = Ещё не прочитано: подключитесь, чтобы прочитать.
database-navigator-nothing-selected = В навигаторе баз данных ничего не выбрано; укажите подключение или папку.
database-navigator-other-database = { $database } — другая база данных сервера; она читается через своё подключение.
database-error-unknown-object = В подключении { $connection } нет объекта { $path }, насколько прочитана его структура.
database-error-not-key-value = Подключение { $connection } — не база ключей и значений; ключи в нём не читаются.
database-error-no-object-tab = Не открыта ни таблица, ни другой объект: откройте его из навигатора баз данных.
database-error-no-object-tab-agent = Нет активной вкладки объекта; откройте её через `db.object.open` или укажите `tab`.
database-error-object-no-data = У { $name } нет строк для показа.
database-error-object-page = У этого объекта нет страницы { $page }.
database-error-object-section = У этого объекта нет раздела «{ $section }»; его разделы: { $sections }.
database-error-no-cell = Нет ячейки в строке { $row }, столбце { $column }: в результате { $rows } строк и { $columns } столбцов.
database-error-save-folder = { $path } — папка; укажите файл.
database-error-save-exists = { $path } уже существует; передайте overwrite, чтобы заменить его.
database-object-section-columns = Столбцы
database-object-section-constraints = Ограничения
database-object-section-foreign-keys = Внешние ключи
database-object-section-indexes = Индексы
database-object-section-triggers = Триггеры
database-object-section-references = Ссылки
database-object-section-policies = Политики
database-object-section-partitions = Секции
database-object-section-statistics = Статистика
database-object-section-permissions = Права
database-object-section-source = Исходный код
database-object-section-settings = Параметры
database-object-section-objects = Объекты
database-object-yes = да
database-object-identity = identity ({ $how })
database-object-generated = вычисляемый: { $expression }
database-object-column-name = Имя
database-object-column-type = Тип
database-object-column-not-null = Not Null
database-object-column-default = По умолчанию
database-object-column-collation = Сортировка
database-object-column-comment = Комментарий
database-object-column-kind = Вид
database-object-column-columns = Столбцы
database-object-column-definition = Определение
database-object-column-references = Ссылается на
database-object-column-on-update = При изменении
database-object-column-on-delete = При удалении
database-object-column-unique = Уникальный
database-object-column-method = Метод
database-object-column-size = Размер
database-object-column-function = Функция
database-object-column-enabled = Включён
database-object-column-schema = Схема
database-object-column-table = Таблица
database-object-column-constraint = Ограничение
database-object-column-command = Команда
database-object-column-permissive = Разрешающая
database-object-column-roles = Роли
database-object-column-using = Using
database-object-column-check = With Check
database-object-column-partition = Секция
database-object-column-bound = Границы
database-object-column-property = Свойство
database-object-column-value = Значение
database-object-column-grantee = Кому
database-object-column-privileges = Права
database-object-column-grantor = Кем выданы
database-object-column-rows = Строки
database-object-primary-key = Первичный ключ
database-object-unique-key = Уникальный ключ
database-object-check = Проверка
database-object-exclusion = Исключение
database-object-foreign-key = Внешний ключ
database-object-partitioned-by = Секционирована по { $key }
database-object-partition-of = Секция таблицы { $parent } { $bound }
database-object-row-estimate = Строки (оценка)
database-object-total-size = Общий размер
database-object-index-count = Индексы
database-object-index-size = Размер индексов
database-object-column-count = Столбцы
database-object-row-security = Защита строк
database-object-data-type = Тип данных
database-object-start = Начало
database-object-increment = Шаг
database-object-minimum = Минимум
database-object-maximum = Максимум
database-object-cache = Кэш
database-object-cycle = Цикл
database-object-owned-by = Принадлежит
database-object-labels = Метки
database-object-base-type = Базовый тип
database-object-checks = Проверки
database-object-owner = Владелец
database-object-tablespace = Табличное пространство
database-object-default-tablespace = (по умолчанию)
database-object-arguments = Аргументы
database-object-returns = Возвращает
database-object-language = Язык

## Фильтр и сортировка данных
database-error-no-active-result = Результат не показан: откройте данные таблицы или выполните запрос в консоли.
database-error-filter-semicolon = Фильтр — это одно условие: уберите «;».
database-error-filter-parentheses = Скобки в фильтре не сходятся.
database-error-filter-unterminated = В фильтре не закрыта кавычка или комментарий.
database-error-filter-not-query = Фильтровать можно только запрос, который читает строки; ищите в загруженных строках.
database-error-filter-pending-changes = Сохраните или отмените изменения данных, прежде чем снова фильтровать или сортировать.
database-error-filter-no-row = Выберите ячейку, по значению которой фильтровать.
database-error-filter-empty = Нет условия, которое можно сохранить.
database-error-columns-pending = Столбцы результата ещё не пришли: дождитесь, пока строки прочитаются.
database-error-reading = Строки читаются заново: дождитесь их, затем меняйте.
database-question-staged = Результатов с несохранёнными изменениями данных: { $count }. Сначала сохраните или отмените изменения.
database-error-no-column = Нет столбца { $column }.
database-error-go-to-row = Строки { $row } нет: прочитано строк: { $rows }; «Следующая страница» или «Прочитать все» прочитают остальные.
database-error-cannot-sort = Эти строки нельзя прочитать заново, и загружены не все: загрузите все строки, чтобы их отсортировать.

## Изменение данных
database-read-only-connection = Только чтение: подключение только для чтения.
database-read-only-driver = Только чтение: результаты этой базы данных нельзя изменять.
database-read-only-no-table = Только чтение: строки взяты не из таблицы.
database-read-only-several-tables = Только чтение: строки соединяют несколько таблиц.
database-read-only-unknown-table = Только чтение, пока не прочитана структура таблицы.
database-read-only-no-key = Только чтение: строки не определяются ключом. Выберите ключ или используйте все столбцы.
database-read-only-no-row-identity = Только чтение: у строк этой базы данных нет идентичности, поэтому изменение не может найти одну строку.
database-error-saving = Изменения сохраняются.
database-error-read-only-column = Столбец { $column } нельзя изменить: он вычисляемый или генерируемый.
database-error-not-null = Столбец { $column } не может быть NULL.
database-error-input-integer = Это не целое число.
database-error-input-number = Это не число.
database-error-input-boolean = Это не true и не false.
database-error-input-json = Это не корректный JSON.
database-error-input-uuid = Это не UUID.
database-error-input-bytes = Это не байты: запишите их как \x и шестнадцатеричные цифры.
database-error-input-object-id = Это не ObjectId: запишите 24 шестнадцатеричные цифры.
database-error-save-conflict = Строку { $row } изменили или удалили до вас: ни одна строка не изменилась. Ничего не сохранено.
database-error-save-several = Сохранение строки { $row } изменило бы несколько строк: они совпадают в столбцах, определяющих строку. Ничего не сохранено.
database-error-save-row = Строка { $row }: { $error } Ничего не сохранено.
database-error-save-kept = Строка { $row }: { $error } Изменения до неё остаются сохранёнными: эта база сохраняет их без транзакции. Обновите результат, чтобы их увидеть.
database-error-save-gone = документ изменён или удалён кем-то другим.
database-error-confirm-data-change = Тип подключения требует подтверждать изменения данных: будет выполнено инструкций — { $count }.
database-error-invalid-key = Ключ составляется из столбцов таблицы в результате: { $columns }.
database-question-pending-one = В { $name } ({ $context }) { $count ->
    [one] { $count } незафиксированная команда, которая могла изменить данные
    [few] { $count } незафиксированные команды, которые могли изменить данные
   *[other] { $count } незафиксированных команд, которые могли изменить данные
}.
database-error-ssh = SSH-туннель не открылся: { $detail }
database-error-unknown-detection = Эта найденная база данных больше не обнаруживается в проекте.
database-error-unknown-detection-agent = В этом проекте нет найденной базы данных { $id }; список даёт `db.detect.list`.
database-error-import-no-path = Укажите папку или файл, из которых импортировать.
database-error-import-none = В { $path } не найден { $file }.
database-error-import-read = Не удалось прочитать { $file }: { $error }
database-error-import-too-large = файл больше 8 МБ
database-import-unsupported = Для { $product } в IDE пока нет драйвера.
database-error-sqlite-missing = Для SQLite нужна программа sqlite3, и она не найдена ({ $detail }). В macOS она есть; в других системах установите SQLite с sqlite.org или менеджером пакетов либо укажите путь к программе на странице «Дополнительно» подключения.
database-error-file-missing = { $detail }: файла базы данных нет. Выберите существующий файл кнопкой «Открыть…» или создайте новый кнопкой «Создать…» на странице «Главное» подключения.
database-file-no-path = Укажите файл базы данных.
database-file-not-absolute = Путь { $path } относительный, а проект не открыт: укажите абсолютный путь.
database-file-exists = { $path } уже существует и не заменён; откройте его кнопкой «Открыть…».
database-file-failed = Не удалось создать { $path }: { $error }
database-file-missing = Файла { $path } нет.
database-error-export-not-table = { $path } — не таблица и не представление, у него нет строк для экспорта.
database-error-export-changes = Экспорт выполняет запрос заново, а этот запрос может изменить данные. Экспортируйте уже загруженные строки или выполните запрос в консоли.
database-error-export-nothing = Выберите хотя бы одну таблицу для экспорта.
database-error-export-delimiter = Разделитель не может быть пустым, переводом строки или кавычкой.
database-error-export-one-path = Путь к файлу задаёт один файл; несколько таблиц экспортируйте в папку с шаблоном имени файла.
database-error-export-binary = Файлы Parquet двоичные: экспортируйте их в файл, а не в буфер обмена.
database-error-export-cells-binary = Выделенные ячейки экспортируются как текст: выберите формат, отличный от Parquet, или экспортируйте все строки.
database-error-export-no-folder = Папки { $path } не существует.
database-error-export-exists = { $count ->
    [one] { $path } уже существует.
    [few] { $count } файла уже существуют, среди них { $path }.
   *[other] { $count } файлов уже существуют, среди них { $path }.
}
database-error-export-is-folder = Папка называется так же, как экспортируемый файл.
database-error-unknown-export = Экспорта { $id } нет.
database-export-title = Экспорт
database-export-cancelled = Экспорт отменён; недописанных файлов не осталось.
database-export-failed = Экспорт не удался:
    { $reasons }
database-export-copied = { $rows ->
    [one] { $rows } строка скопирована в буфер обмена.
    [few] { $rows } строки скопированы в буфер обмена.
   *[other] { $rows } строк скопировано в буфер обмена.
}
database-export-done-file = { $rows ->
    [one] { $rows } строка экспортирована в { $path }.
    [few] { $rows } строки экспортированы в { $path }.
   *[other] { $rows } строк экспортировано в { $path }.
}
database-export-done-files = Строк экспортировано: { $rows }, файлов: { $count }, папка { $folder }.
database-export-open = Открыть
database-export-reveal = Показать в Finder

## Подсказки SQL в консолях
database-sql-kind-table = Таблица
database-sql-kind-foreign-table = Внешняя таблица
database-sql-kind-view = Представление
database-sql-kind-materialized-view = Материализованное представление
database-sql-kind-function = Функция
database-sql-kind-procedure = Процедура
database-sql-kind-sequence = Последовательность
database-sql-kind-type = Тип
database-sql-kind-event = Событие
database-sql-kind-package = Пакет
database-sql-kind-column = Столбец
database-sql-not-null = not null
database-sql-primary-key = первичный ключ
database-sql-not-read = Столбцы ещё не прочитаны: они читаются, когда подключение установлено.
database-sql-more-columns = { $count ->
    [one] …и ещё { $count } столбец
    [few] …и ещё { $count } столбца
   *[other] …и ещё { $count } столбцов
}
database-sql-unknown-table = Неизвестная таблица { $name }
database-sql-unknown-column = Неизвестный столбец { $name }
database-sql-unknown-column-of = Неизвестный столбец { $name } в { $table }
database-error-read-only-statement = Подключение только для чтения: { $verb } не выполнено, потому что может изменить данные или структуру.
database-error-refused = Не выполнено: { $detail }
database-error-read-only-run = { $name } — только для чтения: { $statements } изменило бы данные или структуру, поэтому ничего не выполнено.
database-error-confirm-execute = { $name } — подключение типа «{ $kind }»: { $count ->
    [one] { $count } инструкция меняет
    [few] { $count } инструкции меняют
   *[other] { $count } инструкций меняют
} данные или структуру. Подтвердите выполнение.
database-error-dangerous = Подтвердите перед выполнением в { $name }: { $risks }.
database-confirm-commit = { $commit ->
    [auto] Автофиксация включена: каждое изменение фиксируется сразу.
   *[manual] Ручная фиксация: изменения остаются в открытой транзакции, пока вы не зафиксируете или не откатите их.
}
database-type-kept-manual = Подключение { $name } получило новый тип подключения; { $count ->
    [one] сеанс с незафиксированными изменениями остаётся
   *[other] сеансы с незафиксированными изменениями ({ $count }) остаются
} в ручной фиксации, пока вы не зафиксируете или не откатите их.
database-error-all-skipped = Все инструкции, требовавшие подтверждения, пропущены, а других не осталось.
database-error-agent-not-allowed = Агентам ничего нельзя выполнять в { $name } (тип подключения «{ $kind }»).
database-error-agent-read-only = В { $name } («{ $kind }») агентам можно только читать: { $statements } не выполнено. Человек может проверить и выполнить это: откройте в консоли через db.console.new.
database-error-agent-outside = Агентам можно выполнять SQL только в самой базе { $name }: { $statements } обращается к файлам, расширениям или другим серверам и не выполнено. Человек может проверить и выполнить это: откройте в консоли через db.console.new.
database-error-agent-change = В { $name } («{ $kind }») агентам можно только читать: сохранение, фиксация и изменение ключей остаются человеку.
database-error-agent-loosen = Агентам нельзя ослаблять защиту { $name }: снимать «только чтение», менять тип подключения и подтверждения может только человек.
database-error-agent-loosen-type = Агентам нельзя ослаблять тип подключения { $name }.
database-error-agent-program = Агентам нельзя выбирать программу, которую запускает драйвер (настройка { $setting }): это выбор человека.
database-risk-delete = DELETE без WHERE удалит все строки { $object }
database-risk-delete-unknown = DELETE без WHERE удалит все строки
database-risk-update = UPDATE без WHERE изменит все строки { $object }
database-risk-update-unknown = UPDATE без WHERE изменит все строки
database-risk-drop = DROP удалит { $object } вместе с данными
database-risk-drop-unknown = DROP удалит объект вместе с данными
database-risk-truncate = TRUNCATE удалит все строки { $object }
database-risk-truncate-unknown = TRUNCATE удалит все строки
database-risk-flush = FLUSH удалит все ключи базы
database-error-script-not-bound = { $path } не привязан к подключению: выберите его над редактором.
database-error-script-not-bound-agent = { $path } не привязан к подключению: выберите его через `db.script.bind`.
database-error-code-not-bound = SQL в { $path } не связан с подключением: выберите подключение для этого файла или для всего проекта.
database-error-code-not-bound-agent = SQL в { $path } не связан с подключением: выберите его через `db.injection.bind` или задайте подключение проекта по умолчанию через `db.injection.setDefault`.
database-error-not-code = SQL в строковых литералах ищется в файлах Go, Rust, Python, JavaScript, TypeScript, Java и Kotlin; откройте такой файл или укажите его в `path`.
database-error-no-fragment = У каретки нет строкового литерала с SQL: поставьте каретку внутрь кавычек запроса.
database-error-choose-connection = Выберите подключение для SQL в этом файле: нажмите ⌥⏎ в строке с запросом и выберите «Выбрать подключение для SQL…».
database-error-choose-connection-agent = Выберите подключение для SQL в этом файле через `db.injection.bind` или подключение проекта через `db.injection.setDefault`.
database-injection-run = Выполнить SQL
database-injection-choose-connection = Выбрать подключение для SQL…
database-injection-change-connection = Подключение для SQL: { $name }…
database-error-console-unbind = Консоль всегда работает с подключением: выберите другое.
database-error-script-folder = Скрипты создаются в папке проекта, которая не скрыта: { $path }
database-error-script-exists = { $path } уже существует.
database-error-parameters-needed = { $count ->
    [one] Нужно { $count } значение: укажите его в окне параметров или в `parameters`.
    [few] Нужно { $count } значения: укажите их в окне параметров или в `parameters`.
   *[many] Нужно { $count } значений: укажите их в окне параметров или в `parameters`.
}
database-error-invalid-parameter = Неверное значение { $name }: { $error }
database-error-variable-name = Недопустимое имя переменной: { $name }
database-error-variables-not-saved = Не удалось сохранить переменные: { $error }
database-error-script-command = Ошибка в команде скрипта: { $error }
database-error-include = Не удалось подключить { $file }: { $error }
database-include-missing = такого файла нет ни рядом со скриптом, ни в проекте
database-include-cycle = файл подключает сам себя
database-include-too-deep = вложенность подключений больше { $most }
database-parameters-inlined = Записано в текст команды как литералы в кавычках, так как у этого драйвера нет параметров привязки: { $values }
database-result-on-connection = { $connection } · { $title }
database-error-several-none = Выберите хотя бы одно подключение, на котором выполнить скрипт.
database-error-several-too-many = За раз скрипт выполняется не более чем на { $most } подключениях; выбрано: { $count }.
database-error-several-running = Эта консоль уже выполняется на нескольких подключениях; сначала отмените тот запуск.
database-error-several-no-run = Эта консоль не запускалась на нескольких подключениях.
database-error-several-refused = Ничего не выполнено: { $count ->
    [one] { $count } подключение отказывается
    [few] { $count } подключения отказываются
   *[other] { $count } подключений отказываются
} выполнять скрипт: { $names }. { $reason }
database-error-several-confirm = Подтвердите запуск (подключений: { $count }): { $asking ->
    [one] { $asking } требует
    [few] { $asking } требуют
   *[other] { $asking } требуют
} подтверждения изменений ({ $names }).
database-error-unknown-profile = Профиля баз данных «{ $profile }» нет.
database-notice-profile-detected = Сервер подключения { $name } — { $profile }. Структура читается так, как нужно для { $profile }; настройки подключения не меняются.
database-notice-profile-use = Использовать профиль { $profile }
database-param-error-number = не число: { $text }
database-param-error-boolean = не логическое значение: { $text }
database-param-error-value = не значение параметра: укажите текст, число, логическое значение, null или {"{"} "type", "value" {"}"}
database-param-error-include = после @include нужен файл
database-param-error-set = @set записывается как имя = значение
database-param-error-name = не имя переменной: { $name }
database-param-error-no-value = нет значения
database-error-agent-closed = { $name } закрыто для агентов (тип подключения «{ $kind }»): агенты не видят в нём ничего — ни данных, ни объектов и структуры.

## Удаление подключения

database-remove-question = Удалить подключение «{ $name }»? { $tabs ->
    [0] Ни одна вкладка не закроется.
    [one] Закроется { $tabs } вкладка.
    [few] Закроются { $tabs } вкладки.
   *[many] Закроются { $tabs } вкладок.
}{ $consoles ->
    [0] {""}
    [one] {" "}{ $consoles } консоль останется без подключения.
    [few] {" "}{ $consoles } консоли останутся без подключения.
   *[many] {" "}{ $consoles } консолей останутся без подключения.
}
database-usages-title = Использования { $name }
database-usages-rename-title = Переименование { $name } в { $to }
database-usages-target = { $kind } { $name }
database-usages-rename-target = { $target }, новое имя { $to }
database-usages-scope = проекте и { $connection }
database-usages-consoles = Консоли
database-usages-kind-trigger = Триггер
database-usages-kind-foreign-key = Внешний ключ
database-usages-no-structure = Структура этого подключения ещё не прочитана: сначала подключитесь или обновите его в Database Navigator.
database-usages-unknown = { $name } нет в структуре этого подключения.
database-usages-unsupported = Поиск использований работает для таблиц, представлений, процедур и столбцов.
database-usages-no-new-name = Введите новое имя.
database-folder-aggregates = Агрегатные функции
database-folder-extensions = Расширения
database-folder-event-triggers = Событийные триггеры
database-folder-tablespaces = Табличные пространства
database-folder-roles = Роли
database-object-no = нет
database-object-section-dependencies = Зависимости
database-object-section-rules = Правила
database-object-column-direction = Направление
database-object-column-instead = Вместо
database-object-uses = использует
database-object-used-by = используется в
database-object-primary = Первичный ключ
database-object-version = Версия
database-object-relocatable = Переносимое
database-object-event = Событие
database-object-tags = Теги команд
database-object-location = Расположение
database-object-options = Параметры
database-object-login = Может входить
database-object-superuser = Суперпользователь
database-object-create-database = Может создавать базы данных
database-object-create-role = Может создавать роли
database-object-replication = Репликация
database-object-bypass-row-security = Обходит защиту строк
database-object-connection-limit = Лимит подключений
database-object-no-limit = без ограничения
database-object-valid-until = Пароль действует до
database-object-member-of = Входит в роли
