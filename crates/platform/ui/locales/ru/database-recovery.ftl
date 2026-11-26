# Zen UI: the Recovered tab of the Database workspace.

ide-dbrecovery-note = Сохранено, когда IDE завершилась, не закрывшись. Ничего не выполнялось в базе данных.
ide-dbrecovery-discard-all = Отбросить всё
ide-dbrecovery-empty = Восстанавливать нечего.
ide-dbrecovery-loading = Чтение сохранённого…
ide-dbrecovery-kind-console = Несохранённая консоль
ide-dbrecovery-kind-edits = Несохранённые изменения данных
ide-dbrecovery-kind-transaction = Откаченная транзакция
ide-dbrecovery-lines = { $count ->
    [one] { $count } строка
    [few] { $count } строки
   *[other] { $count } строк
}
ide-dbrecovery-rows = { $count ->
    [one] { $count } строка данных
    [few] { $count } строки данных
   *[other] { $count } строк данных
}
ide-dbrecovery-statements = { $count ->
    [one] { $count } запрос, изменяющих данные: { $modifying }
    [few] { $count } запроса, изменяющих данные: { $modifying }
   *[other] { $count } запросов, изменяющих данные: { $modifying }
}
ide-dbrecovery-restore = Восстановить
ide-dbrecovery-open-sql = Открыть SQL в консоли
ide-dbrecovery-discard = Отбросить
ide-dbrecovery-dismiss = Скрыть
ide-dbrecovery-console-explained = Несохранённый текст консоли. «Восстановить» откроет консоль с ним как с несохранёнными изменениями; отмена вернёт сохранённый текст.
ide-dbrecovery-edits-explained = Запросы, которые сохранили бы подготовленные изменения. Каждое обновление находит строку по старому ключу и проверяет, что прочитанные значения на месте. Откройте SQL в консоли, чтобы проверить и выполнить их вручную.
ide-dbrecovery-transaction-explained = Эта транзакция была открыта, когда IDE завершилась. Сервер откатил её при закрытии соединения, поэтому её изменений в базе нет. Последние запросы:
ide-dbrecovery-discard-question = Отбросить эту восстановленную работу? Вернуть её потом будет нельзя.
ide-dbrecovery-discard-all-question = Отбросить всю восстановленную работу? Вернуть её потом будет нельзя.
ide-dbrecovery-cancel = Отмена
