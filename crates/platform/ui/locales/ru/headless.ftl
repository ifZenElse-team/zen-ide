# Zen UI: the headless IDE for AI agents (ADR-0084), Russian.

ide-headless-window-only = `{ $tool }` недоступен в IDE без окна: клавишам нужно нарисованное окно. Вызовите действие клавиши через run_action или команду через dispatch_command (их перечисляют find_action и list_commands).
ide-headless-running = { $project } уже открыт в { $product } без окна (процесс { $pid }) по адресу { $url }
ide-headless-no-server = не удалось запустить сервер MCP на порту { $port }
ide-headless-usage = { $error }; использование: { $program } --headless [--mcp-port <порт>] [--exit-with-parent] [--accept-open] [--no-restore] [<проект>...]
ide-headless-bad-port = `{ $value }` — не номер порта
ide-headless-no-port = для --mcp-port нужен порт
ide-headless-unknown = неизвестный параметр `{ $option }`
ide-headless-servers-found = { $project }: найдены языковые серверы: { $servers }
ide-headless-servers-none-found = { $project }: ни один нужный проекту языковой сервер не установлен
ide-headless-server-missing = { $project }: { $server } не установлен; его устанавливает lsp.server.install с id { $server } (выполняет { $command })
ide-headless-server-missing-manual = { $project }: { $server } не установлен: { $hint }
ide-headless-safe-mode = { $project } открыт в безопасном режиме: его языковые серверы и программы не запускаются. Агент не может это изменить, и командной строкой этого не сделать: человек открывает проект один раз в окне IDE с этой же папкой данных и доверяет ему там.
