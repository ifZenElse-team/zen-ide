http-error-not-http = { $path } — не файл HTTP-запросов (.http или .rest).
http-error-which-request = В файле несколько запросов: укажите номер или имя запроса.
http-error-no-request = В { $path } нет запроса { $request }.
http-error-no-environment = Окружения «{ $env }» нет ни в http-client.env.json, ни в http-client.private.env.json.
http-error-env-file = Не удалось прочитать { $path }: { $error }
http-error-unresolved = Не заданы переменные: { $names }. Задайте их в файле (@name = value) или в http-client.env.json и выберите окружение.
http-error-body-file = Не удалось прочитать { $path } для тела запроса: { $error }
http-error-no-url = У запроса нет URL.
http-error-send = Не удалось отправить запрос: { $error }
http-error-unavailable = Здесь запросы отправлять нельзя.
http-error-no-response = Ответа { $id } нет.
http-error-body-outside = { $path } — вне проекта: тело запроса читается только из файлов проекта.
http-error-scheme = Отправляются только запросы http и https, а не { $scheme }.
http-error-redirect-scheme = сервер перенаправил на адрес { $scheme }, по такому перенаправлению запрос не идёт
http-error-redirects = сервер перенаправил больше 10 раз
http-error-timeout = за минуту ответа не было
