# Окна локальной истории (zen_ui::ide::local_history, ADR-0060).

ide-local-history-title = История { $name }
ide-local-history-empty = Локальной истории пока нет
ide-local-history-no-changes = С этой версии файлы не менялись
ide-local-history-no-diff = Сравнивать нечего
ide-local-history-revert = Вернуть
ide-local-history-close = Закрыть
ide-local-history-reverted = { $count ->
    [0] Возвращать нечего: файлы такие же, как были
    [one] Возвращён { $count } файл
    [few] Возвращено { $count } файла
   *[other] Возвращено { $count } файлов
}
ide-local-history-modified = изменён
ide-local-history-deleted = удалён
ide-local-history-added = создан позже
ide-local-history-today = Сегодня { $time }
ide-local-history-yesterday = Вчера { $time }
ide-local-history-label-title = Поставить метку
ide-local-history-label-name = Название метки:
ide-local-history-label-ok = ОК
ide-local-history-label-cancel = Отмена
