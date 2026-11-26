# Отладчик (ADR-0052): сеансы отладки, точки останова и наблюдения.

debug-cannot-debug = Конфигурацию «{ $name }» нельзя отлаживать.
debug-cannot-start = Не удалось начать отладку «{ $name }»: { $error }
debug-debugger-missing = Не удалось начать отладку «{ $name }»: отладчик не установлен.
debug-no-session = Нет сеанса отладки.
debug-unknown-session = Нет сеанса отладки { $id }.
debug-session-ended = Сеанс отладки { $id } завершён.
debug-not-connected = Отладчик ещё не подключился.
debug-not-stopped = Программа не остановлена.
debug-no-thread = У программы нет потоков.
debug-no-frame = Кадр стека не выбран.
debug-no-hover = Отладчик не вычисляет значения при наведении.
debug-no-set-value = Этот отладчик не может изменить это значение.
debug-no-smart-step = Этот отладчик не умеет перечислять вызовы для захода.
debug-no-reset-frame = Этот отладчик не умеет сбрасывать фреймы.
debug-frame-cannot-reset = Фрейм { $id } нельзя сбросить.
debug-no-source = У кадра, где остановилась программа, нет исходного файла.
debug-unknown-frame = Нет кадра стека { $id }.
debug-no-line = Не указана строка: передайте строку или поставьте курсор в файл.
debug-listen-failed = Не удалось ожидать подключения отладчика: { $error }
debug-launch-failed = Не удалось запустить отладчик: { $error }
debug-connect-failed = Отладчик не подключился: { $error }
debug-handshake-failed = Отладчику не удалось запустить программу: { $error }
debug-adapter-error = Ошибка отладчика: { $error }
debug-disconnected = Отключено от отладчика
debug-server-adapter = Подключение к отладчику, который запускает { $server }, по адресу { $address }
debug-server-adapter-no-port = Языковой сервер не сообщил порт отладчика: { $answer }
debug-no-breakpoint = Нет точки останова в строке { $line } файла { $path }.
debug-empty-watch = Для наблюдения нужно выражение.
debug-unknown-watch = Нет наблюдения «{ $expression }».
debug-error-io = не удаётся связаться с отладчиком: { $error }
debug-error-closed = отладчик не запущен
debug-error-timeout = отладчик не ответил вовремя
debug-reload-title = Перезагрузка изменённых классов
debug-reload-done = { $count ->
    [one] Перезагружен { $count } класс: { $classes }
    [few] Перезагружено { $count } класса: { $classes }
   *[other] Перезагружено { $count } классов: { $classes }
}
debug-reload-none = С последней загрузки классы не менялись.
debug-reload-failed = Изменённые классы не перезагружены: { $error }
debug-reload-unsupported = Этот отладчик не умеет перезагружать изменённые классы.
debug-log-hit = Точка останова достигнута в { $place }
debug-cannot-move = В строке { $line } уже есть точка останова.
debug-empty-function = Для точки останова на функции нужно имя функции.
debug-no-function = Нет точки останова на функции «{ $name }».
debug-empty-filter = Для точки останова на исключениях нужен фильтр.
debug-no-filter = Нет точки останова на исключениях «{ $filter }».
debug-no-data-breakpoints = Этот отладчик не умеет следить за данными.
debug-cannot-watch = За этим нельзя следить: { $reason }
debug-no-data = Нет точки останова на данных «{ $id }».
debug-stream-java-only = Трассировка цепочки потока работает в файлах Java.
debug-stream-none = В этой строке нет цепочки потока.
debug-stream-not-created = Поток создаётся не в этой строке: поток из переменной можно использовать только один раз, поэтому трассируйте строку, где он создаётся, например list.stream()….
debug-stream-no-operations = У потока в этой строке нет операций для трассировки.
debug-stream-side-effect = Цепочка вызывает { $call } с побочными эффектами: трассировка выполнила бы их ещё раз.
debug-stream-infinite = Поток бесконечный: добавьте limit(…) или takeWhile(…), чтобы его трассировать.
debug-coroutines-no-event = Корутины читаются в потоке, который остановила точка останова или шаг, а не пауза: сделайте шаг или остановитесь на точке останова.
debug-coroutines-not-jvm = Корутины показываются для программ на Java и Kotlin.
debug-coroutines-not-installed = В программе нет агента отладки корутин: включите «Отслеживать корутины» в её конфигурации запуска и запустите отладку снова.
debug-coroutines-no-library = В classpath программы нет kotlinx-coroutines-debug.
debug-coroutines-no-frame = Ни один из верхних фреймов ({ $count }) не может прочитать корутины: зайдите в код самой программы и посмотрите снова.
