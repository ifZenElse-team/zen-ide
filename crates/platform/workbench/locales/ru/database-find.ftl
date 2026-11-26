# Zen Workbench: поиск в базе данных, поиск по таблицам схемы на сервере.

database-find-title = Поиск в базе данных
database-find-warning-production = Подключение типа «{ $kind }»: поиск прочитает на сервере каждый выбранный столбец каждой таблицы.
database-find-warning-large = В таблицах { $schema } около { $rows } строк: поиск нагрузит сервер; каждая таблица останавливается на своём пределе и сроке.
database-find-warning-views = Чтобы искать в представлениях, выполняются их запросы, а это бывает долго.
database-error-search-not-sql = { $name } не использует SQL, поэтому искать в нём нельзя.
database-error-search-no-text = Введите текст для поиска.
database-error-search-nothing = Выберите, где искать: в данных, в именах или и там и там.
database-error-search-no-schema = Выберите схему: структура подключения ещё не прочитана.
database-error-unknown-search = Поиска { $id } нет.
database-error-no-search = Поиск ещё не запускался.
database-error-search-unknown-table = Поиск не просматривал { $table }.
database-error-search-no-columns = Ни в одном из этих столбцов текста нет.
