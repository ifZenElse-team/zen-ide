# Zen Workbench: инструменты миграций проекта в рабочей области «База данных».

database-migration-title = Миграции
database-migration-read-failed = Не удалось прочитать таблицу применённых миграций.
database-migration-ssh = { $name } подключается через SSH, который IDE открывает только для себя: инструменты миграций до неё не дотянутся. Запустите их из терминала через свой туннель.
database-migration-driver = { $name }: инструменты миграций работают только с PostgreSQL, MySQL, MariaDB и SQLite, не с { $driver }.
database-migration-no-down = { $tool } не умеет откатывать миграции; напишите новую миграцию.
database-migration-needs-state = { $tool } откатывает до названной миграции, поэтому сначала нужно прочитать применённые: подключитесь и откройте вкладку «Миграции».
database-migration-unknown-tool = { $tool } — неизвестный IDE инструмент миграций.
database-migration-unknown-direction = { $direction } — не направление: укажите up или down.
database-migration-no-folder = В { $folder } больше нет миграций { $tool }.
database-migration-password-unread = Пароль { $name } ещё не прочитан: запустите миграцию один раз со вкладки «Миграции» рабочей области «База данных» — она прочитает его из связки ключей.
database-migration-not-found = { $program } не найден в PATH; установите { $tool }, чтобы запускать его миграции.
database-migration-unknown-set = { $id } больше не папка миграций этого проекта; найденные папки показаны на вкладке «Миграции».
database-migration-unknown-set-agent = { $id } — не папка миграций этого проекта; список даёт `db.migration.list`.
database-migration-name-up = { $tool } up · { $name }
database-migration-name-down = { $tool } down · { $name }
database-migration-confirm-up = Применить к { $name } ожидающие миграции { $tool } ({ $count })?
database-migration-confirm-down = Откатить последнюю миграцию { $tool } ({ $migration }) на { $name }?
database-migration-risk-type = Подключения типа { $kind } спрашивают перед любым изменением.
database-migration-risk-down = Откат миграции обычно удаляет таблицы или столбцы вместе с их данными.
database-migration-read-only = { $name } только для чтения: миграции её меняют, поэтому ничего не запущено.
database-migration-agent-read-only = На { $name } ({ $kind }) агенты могут только читать: миграции там запускает человек со вкладки «Миграции».
database-migration-nothing-applied = Ни одна миграция { $tool } не применена — откатывать нечего.

workbench-dbmigration-kind = Миграция базы данных
workbench-dbmigration-field-tool = Инструмент
workbench-dbmigration-field-folder = Папка миграций
workbench-dbmigration-field-connection = Подключение
workbench-dbmigration-field-direction = Направление
workbench-dbmigration-field-target = Откатить до (Django)
database-migration-password-in-url = { $tool } принимает пароль только в URL или в переменных окружения, где его могут прочитать другие программы, поэтому IDE его не передаёт. Запустите инструмент в терминале со своей конфигурацией или используйте подключение без пароля.
database-migration-bad-name = { $name } — не имя приложения: оно было бы прочитано как параметр.
database-migration-not-approved = Запускайте миграции из вкладки «Миграции» (Run Migrations): она проверяет подключение и спрашивает подтверждение; сохранённая конфигурация или повторный запуск этого не делают.
database-migration-password-unsupported = Передача пароля инструментам миграций на этой системе пока не поддерживается.
database-migration-not-connected = Сначала подключитесь к { $name }: то, что будет выполнено, берётся из уже применённых миграций.
