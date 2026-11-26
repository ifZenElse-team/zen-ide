# Zen Workbench: консольный клиент подключения в Terminal и типы, созданные по таблицам.

database-client-tab = { $tool }: { $name }
database-client-not-found = Консольный клиент { $name } ({ $tool }) не установлен или его нет в PATH.
database-client-unsupported = У { $name } нет консольного клиента, который IDE может запустить.
database-client-tunnel-closed = { $name } работает через SSH-туннель: сначала подключитесь, затем откройте клиент.
database-client-tunnel-unsupported = Консольный клиент { $name } не может работать через его SSH-туннель.
database-client-sentinel = { $name } — развёртывание Redis Sentinel; откройте клиент на его основном узле.
database-client-no-file = У { $name } не указан файл базы данных.
database-error-no-columns = У { $name } нет столбцов, из которых можно сделать тип.
database-error-columns-not-read = Столбцы { $name } ещё не прочитаны: подключитесь и повторите.
database-client-secret-file = Не удалось записать файл с паролем для клиента: { $error }
database-client-agent-guarded = Агенты не открывают консольный клиент { $name }: подключение только для чтения или спрашивает перед изменениями, а клиент выполняет всё, что в него введено, не спрашивая.
