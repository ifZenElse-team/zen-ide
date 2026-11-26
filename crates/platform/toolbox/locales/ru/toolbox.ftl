# Zen Toolbox: назначение инструментов и сообщения. Перевод с английского.

toolbox-subject-rustronin = Rust
toolbox-subject-gogarden = Go
toolbox-subject-pypetal = Python
toolbox-subject-jvmjizo = для Java и JVM
toolbox-subject-webwave = Веб: TypeScript, JavaScript, HTML и CSS
toolbox-subject-cppcrane = C и C++
toolbox-subject-sharpshuriken = C# и .NET
toolbox-subject-phppagoda = PHP
toolbox-subject-rubyryokan = Ruby
toolbox-subject-sqlscroll = SQL и базы данных
toolbox-subject-devopsdaruma = DevOps: контейнеры, инфраструктура как код, пайплайны
toolbox-subject-testtotami = QA: сквозные и UI-тесты, тестирование API и нагрузки, управление тестированием

toolbox-error-not-initialised = Zen Toolbox не инициализирован.
toolbox-error-demo = Это демонстрационные данные: ничего нельзя установить или запустить.
toolbox-error-no-tools-dir = Не задана папка для инструментов.
toolbox-error-unknown-tool = Инструмента «{ $id }» нет.
toolbox-error-no-server = Не задан сервер релизов.
toolbox-error-not-released = { $tool } ещё не вышел.
toolbox-error-up-to-date = У { $tool } последняя версия.
toolbox-error-not-installed = { $tool } не установлен.
toolbox-error-cannot-start = Не удалось запустить { $tool }: { $error }
toolbox-error-no-folder = Папка «{ $path }» не найдена.
toolbox-error-not-a-folder = «{ $path }» — не папка.
toolbox-error-unknown-project = Ни одна Zen IDE не открывала «{ $path }»; выберите, в какой IDE её открыть.
toolbox-error-self-up-to-date = У Zen Toolbox последняя версия.
toolbox-error-no-manifest = Список релизов ещё не загружен.
toolbox-error-no-executable = Не удалось найти исполняемый файл Zen Toolbox.
toolbox-error-no-build = Сборки { $tool } для этой платформы нет.
toolbox-error-busy = { $tool } уже устанавливается.
toolbox-error-version-installed = { $tool } { $version } уже установлен.
toolbox-error-no-version = На этом канале нет { $tool } { $version } для этой платформы.

toolbox-uninstalled = { $tool } удалён.
toolbox-installed = { $tool } { $version } установлен.
toolbox-install-failed = Не удалось установить { $tool }: { $error }
toolbox-self-installed = Zen Toolbox { $version } установлен. Перезапустите его, чтобы начать пользоваться.
toolbox-self-failed = Не удалось обновить Zen Toolbox: { $error }

toolbox-settings-error-value = «{ $value }» — недопустимое значение { $key }. Допустимые значения: { $expected }.
toolbox-settings-error-relative-path = { $key } должен быть абсолютным путём, а не «{ $path }».
toolbox-settings-error-write = Не удалось сохранить { $key }: { $error }

toolbox-error-no-shell-scripts-location = Папка для скриптов запуска не задана, а домашняя папка неизвестна.
toolbox-error-launcher-directory = Не удалось создать папку для скриптов запуска { $path }: { $error }
toolbox-error-launcher-write = Не удалось записать скрипт запуска { $path }: { $error }
toolbox-error-launcher-remove = Не удалось удалить скрипт запуска { $path }: { $error }
toolbox-error-launcher-path = Путь { $path } нельзя записать в скрипт запуска.
toolbox-error-launcher-relative = Папка для скриптов запуска должна быть абсолютным путём или начинаться с ~, а не «{ $path }».
toolbox-shell-scripts-failed = Не удалось обновить скрипты запуска: { $error }
toolbox-error-no-home = Домашняя папка неизвестна, поэтому некуда добавить строку профиля оболочки.
toolbox-error-profile = Не удалось добавить строку в профиль оболочки: { $error }
toolbox-error-person-chooses = ИИ-агент не может этого сделать: выбрать версию или включить автоматические обновления может только человек в Zen Toolbox.

toolbox-login-failed = Не удалось изменить запуск при входе: { $error }
toolbox-login-not-ours = { $path } записан не Zen Toolbox, поэтому его не трогаем.
toolbox-login-io = { $path }: { $error }

toolbox-notice-title = Доступен { $tool } { $version }
toolbox-notice-body = Откройте Zen Toolbox, чтобы обновить.
toolbox-updated-title = { $tool } обновлён до { $version }
toolbox-updated-body = Обновление прошло само, как задано в Zen Toolbox.
toolbox-update-failed-title = Не удалось обновить { $tool } до { $version }
toolbox-update-failed-body = { $error }
