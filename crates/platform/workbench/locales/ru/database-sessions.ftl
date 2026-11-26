# Zen Workbench: сеансы и блокировки сервера подключения в рабочей области «База данных».

database-sessions-title = Сеансы: { $name }
database-error-no-sessions = { $name } не показывает сеансы своего сервера; это умеют PostgreSQL, MySQL и MariaDB.
database-error-session-id = { $id } — не номер сеанса этого сервера.
database-error-session-read-only = { $name } только для чтения: отмена запроса или завершение сеанса меняют сервер, поэтому ничего не отправлено.
database-error-agent-session = На { $name } ({ $kind }) агенты могут только читать: отменять запросы и завершать сеансы может только человек.
database-error-own-session = Сеанс { $id } — один из сеансов этой IDE ({ $context }); подтвердите, что он ваш, чтобы остановить его.
database-session-confirm-cancel = Отменить выполняемый запрос сеанса { $id }{ $who } на { $name }? Сеанс останется открытым.
database-session-confirm-terminate = Завершить сеанс { $id }{ $who } на { $name }?
database-session-risk-own = Сеанс { $id } — один из сеансов этой IDE.
database-session-risk-terminate = Завершение сеанса { $id } откатит его открытую транзакцию и закроет его подключение.
database-session-risk-query = Он выполняет: { $query }
