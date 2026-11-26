# Инструменты качества кода (ADR-0136)
code-quality-none = Нет линтеров и форматеров: установите плагин ESLint, Prettier или Biome — IDE предлагает его, когда проект использует инструмент.
code-quality-automatic = Автоматическая настройка { $tool }
code-quality-disabled = Отключить { $tool }
code-quality-not-found = { $tool } не установлен
code-quality-in-project = { $tool } запускается из node_modules проекта
code-quality-installed = { $tool } установлен
code-quality-fix-on-save = Применять исправления { $tool } при сохранении
code-quality-formatter-found = Пакет { $tool }: { $path }
code-quality-formatter-missing = { $tool } нет в node_modules проекта: выполните npm install
code-quality-on-reformat = Запускать при Reformat Code
code-quality-on-save = Запускать при сохранении
code-quality-files = Запускать для файлов: { $extensions }
code-quality-bad-mode = Неизвестный режим { $mode }: используйте on или off
code-quality-page = Инструменты качества кода
workbench-format-formatter-failed = { $tool } не смог отформатировать файл: { $error }
workbench-format-formatter-stopped = форматер завершился без ответа
workbench-format-formatter-timeout = он не закончил вовремя
