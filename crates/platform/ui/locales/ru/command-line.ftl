# Zen UI: an IDE's command line (ADR-0085), Russian.

ide-cli-version = { $product } { $version }
ide-cli-about = { $product } { $version }, IDE семейства Zen
ide-cli-usage = Использование:
ide-cli-help-window-syntax = [<папка или файл>...]
ide-cli-help-window = Открывает папки как проекты, каждый в своём окне, и файлы (`файл:строка[:столбец]` — на этом месте); без них — проекты, открытые при последнем выходе. Если IDE уже запущена, открывает их в ней, а эта команда сразу завершается.
ide-cli-help-wait-syntax = --wait <файл>...
ide-cli-help-wait = Открывает файлы и завершается, когда их вкладки закрыты; запускает IDE, если она не запущена. Для `git config core.editor "{ $program } --wait"`.
ide-cli-help-headless-syntax = --headless [--mcp-port <порт>] [--exit-with-parent] [--accept-open] [--no-restore] [<проект>...]
ide-cli-help-headless = Запускает IDE без окна для ИИ-агентов; печатает адрес своего сервера MCP. С --accept-open принимает то, что передаёт следующий запуск, как это делает окно; с --no-restore проекты открываются без прошлых вкладок и окон инструментов.
ide-cli-help-mcp-headless-syntax = mcp --headless [--no-restore] [<проект>]
ide-cli-help-mcp-headless = MCP через стандартные ввод и вывод с IDE без окна для проекта (по умолчанию — текущей папки): она запускается с сеансом агента и завершается с последним из них. Для `claude mcp add -s project <имя> -- <программа> mcp --headless .`
ide-cli-help-mcp = MCP через стандартные ввод и вывод с окном IDE, которая запускается при необходимости.
ide-cli-help-version = Печатает версию.
ide-cli-help-help = Печатает эту справку.
ide-cli-mcp-usage = { $error }; использование: { $program } mcp [--headless [--no-restore] [<проект>]]
ide-cli-mcp-one-project = можно указать только один проект
ide-cli-mcp-project-needs-headless = проект указывается только вместе с --headless
ide-cli-no-data-folder = папка данных приложения неизвестна
ide-cli-cannot-start = не удалось запустить IDE без окна: { $error }
ide-cli-window-usage = { $error }; использование: { $program } [<папка или файл>...]; { $program } --help перечисляет все способы запуска
ide-cli-not-handed-over = ни одна запущенная { $product } с этой папкой данных не приняла папки и файлы, а ZEN_HANDOFF_ONLY запрещает новое окно
ide-cli-wait-no-ide = IDE не запустилась за минуту
