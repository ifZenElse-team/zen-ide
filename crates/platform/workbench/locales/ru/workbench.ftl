# Zen Workbench: сообщения о проектах и файлах. Перевод с английского.

workbench-error-no-project = Проект не открыт.
workbench-error-no-document = Файл не открыт.
workbench-module-unknown = Модуля с именем или в папке { $module } нет.
workbench-error-tabs-sorted = Вкладки упорядочены по алфавиту, поэтому их нельзя переставить: выключите сортировку по алфавиту в Settings → Editor → General → Editor Tabs.
workbench-error-no-line = В { $path } нет строки { $line }: строк в файле { $lines }.
workbench-error-bad-color = { $color } — не цвет: укажите #rrggbb или #rrggbbaa.
workbench-error-clipboard-entry = В истории буфера обмена нет записи { $index }; записей: { $count } (с 0 для самой новой).
workbench-error-no-split = Здесь нет вкладки, которую можно разделить.
workbench-error-no-opposite = Редактор не разделён: нет другой группы, куда перенести вкладку.
workbench-error-no-closed-tab = Нет закрытых вкладок, которые можно открыть снова.
workbench-error-not-split = Редактор не разделён.
workbench-error-no-group = Нет группы редакторов { $group }.
workbench-error-not-a-folder = { $path } — не папка.
workbench-error-exclusion = Не удалось изменить пометку { $path }: { $error }
workbench-error-exclude-patterns = Не удалось изменить шаблоны исключаемых файлов: { $error }
workbench-error-open = Не удалось открыть { $path }: { $error }
workbench-error-save = Не удалось сохранить { $file }: { $error }
workbench-error-changed-on-disk = файл изменён на диске после того, как его прочитали. Выберите в редакторе «Загрузить с диска» или «Оставить мои правки».
workbench-read-only-locked = Файл { $file } доступен только для чтения.
workbench-read-only-offer = Файл { $file } доступен только для чтения, его нельзя править.
workbench-read-only-make-writable = Сделать доступным для записи
workbench-read-only-failed = Не удалось изменить атрибут «только для чтения» у { $file }: { $error }
workbench-read-only-no-file = { $path } — не файл.
workbench-saved = Файл { $file } сохранён.
workbench-error-outside-project = { $path } не является папкой открытого проекта.
workbench-error-separators-outside = { $path } — не открытый файл и не файл или папка открытого проекта.
workbench-error-separators-not-text = { $file } — не текстовый файл в UTF-8: его разделители строк остаются как есть.
workbench-error-separators-read-only = { $file } — только для чтения: его разделители строк остаются как есть.
workbench-error-separators-link-or-large = { $file } — ссылка или файл больше 16 МБ: его разделители строк остаются как есть.
workbench-error-bad-name = «{ $name }» — недопустимое имя.
workbench-error-create = Не удалось создать { $path }: { $error }
workbench-error-copy = Не удалось скопировать { $path }: { $error }
workbench-copied = { $count ->
    [one] Скопирован { $count } элемент.
    [few] Скопировано { $count } элемента.
   *[other] Скопировано { $count } элементов.
}
workbench-error-rename = Не удалось переименовать { $path }: { $error }
workbench-error-move = Не удалось переместить { $path }: { $error }
workbench-moved = { $count ->
    [one] Перемещён { $count } элемент.
    [few] Перемещено { $count } элемента.
   *[other] Перемещено { $count } элементов.
}
workbench-paste-nothing = Нет файлов для вставки.
workbench-error-nothing-selected = Ничего не выбрано.
workbench-error-no-such-file = { $path } не существует.
workbench-error-no-image = В активной вкладке не изображение.
workbench-error-zoom = Укажите действие масштаба или сам масштаб.
workbench-error-delete = Не удалось удалить { $path }: { $error }
workbench-deleted = { $count ->
    [one] Перемещено в Корзину.
    [few] { $count } объекта перемещены в Корзину.
   *[other] { $count } объектов перемещено в Корзину.
}
workbench-error-search = Поиск задан неверно: { $error }
workbench-error-preview-place = Этого места в файле уже нет; откройте совпадение заново.
workbench-error-places-from-one = Строки и столбцы считаются с 1.
workbench-error-preview-read-only = Файл { $file } только для чтения, предпросмотр не может его изменить.
workbench-error-find-no-selection = Выделите текст, в котором искать
workbench-error-find-no-query = Нечего искать
workbench-find-no-occurrences = Вхождений не найдено
workbench-error-no-comment = У этого типа файлов нет такого комментария; укажите его явно.

workbench-structure-no-outline = У этого файла нет структуры.

workbench-run-file-broken = Не удалось прочитать конфигурации запуска из { $path }: { $error }
workbench-run-file-save = Не удалось сохранить конфигурации запуска в { $path }: { $error }
workbench-run-no-configuration = Нет конфигурации запуска, которую можно запустить.
workbench-run-unknown-configuration = Конфигурации запуска «{ $id }» нет.
workbench-run-unknown-kind = Ни один поставщик не запускает конфигурации вида «{ $kind }».
workbench-run-unknown-kind-of = Ни один поставщик не запускает конфигурации вида «{ $kind }»; есть виды: { $kinds }.
workbench-run-unknown-options = У конфигураций вида «{ $kind }» нет параметров { $unknown }; их параметры: { $keys }.
workbench-run-no-context = Здесь нечего запускать.
workbench-run-not-scratch = { $path } не черновик.
workbench-run-scratch-cannot-run = { $path } нельзя запустить отдельно: его язык не запускает отдельные файлы или в нём нет точки входа.
workbench-run-anything-empty = Введите команду или имя конфигурации запуска.
workbench-run-unknown-run = Запуска { $id } нет.
workbench-run-unknown-test = В запуске нет теста { $test }.
workbench-run-cannot-start = Не удалось запустить «{ $name }»: { $error }
workbench-run-already-running = «{ $name }» уже запущена. Остановить и запустить заново?
workbench-run-prompt-needed = Не удалось запустить «{ $name }»: для { $tokens } нужно значение; передайте его в prompt или filePrompt.
workbench-run-no-value = Не удалось запустить «{ $name }»: у { $token } нет значения ({ $reason }).
workbench-run-env-file = Не удалось запустить «{ $name }»: не читается файл переменных окружения { $path }: { $error }
workbench-run-nothing-to-start = конфигурация не запускает ни одной программы
workbench-run-no-test-source = Исходный код теста { $test } не найден.
workbench-run-no-failed = В этом запуске нет упавших тестов.
workbench-run-cannot-rerun-failed = Конфигурации вида «{ $kind }» не умеют перезапускать упавшие тесты.
workbench-run-process-finished = Процесс завершён с кодом выхода { $code }
workbench-run-process-stopped = Процесс остановлен
workbench-run-process-interrupted = Процесс завершён: остановлен сигналом
workbench-run-adopted-read-only = «{ $name }» взята из { $file } среды JetBrains, и здесь её не изменить; чтобы изменить, сохраните её как конфигурацию Zen.
workbench-run-not-adopted = «{ $id }» — не конфигурация, взятая из среды JetBrains.
workbench-run-kind-compound = Составная
workbench-run-compound-cycle = «{ $name }» нельзя сохранить или запустить: составная конфигурация «{ $member }» содержит саму себя.
workbench-run-compound-missing = Составная конфигурация «{ $name }» ссылается на несуществующую конфигурацию: { $id }.
workbench-run-compound-empty = В составной конфигурации «{ $name }» нет конфигураций для запуска.
workbench-run-compound-no-debug = Ни одну из конфигураций составной «{ $name }» нельзя отладить.
workbench-run-template-unknown-kind = Нет вида конфигураций запуска «{ $kind }».
workbench-run-services-nothing = Укажите вид конфигураций запуска или конфигурацию.

workbench-language-restarted = Языковой сервер { $server } остановился и был запущен снова.
workbench-language-stopped = Языковой сервер { $server } остановлен: { $reason }
workbench-language-not-running = Языковой сервер не запущен.
workbench-lsp-unknown = Языкового сервера { $id } нет.
workbench-lsp-not-configurable = Языковой сервер { $id } встроен в эту IDE, у него нет настроек.
workbench-lsp-missing = { $server } не установлен.
workbench-lsp-installing = Установка: { $detail }
workbench-lsp-install-failed = Установка не удалась: { $detail }
workbench-lsp-install-not-found = Установка завершилась, но { $server } не найден.
workbench-lsp-no-alternative = { $id } — единственный сервер своего языка: выбирать не из чего.
workbench-lsp-install-cannot-run = Не удаётся запустить установщик: { $detail }
workbench-lsp-java-missing = Для запуска { $server } нужна Java { $version } или новее: укажите такой JDK для проекта или в JAVA_HOME.
workbench-lsp-install-downloading-from = Загрузка { $url }
workbench-lsp-install-downloaded-of = Загрузка: { $done } из { $total } МБ
workbench-lsp-install-downloaded = Загрузка: { $done } МБ
workbench-lsp-install-unpacking = Проверка и распаковка в { $folder }
workbench-lsp-save-failed = Не удаётся сохранить настройки языковых серверов: { $detail }
workbench-lsp-stopped-by-user = Остановлен пользователем.
workbench-language-not-started = Языковой сервер не запустился.
workbench-language-server-missing = { $server } не установлен, поэтому ответить не может.
workbench-language-server-starting = { $server } запускается; попробуйте чуть позже.
workbench-language-server-not-started = { $server } не запущен: откройте один из его файлов, чтобы запустить его.
workbench-language-server-untrusted = { $server } не запускается в проекте, которому не доверяют: его сборка может выполнить код. Человек может доверять проекту командой Trust Project.
workbench-problems-not-checked = Языковой сервер ещё не проверил этот файл: откройте его (file.document.open), чтобы он был проверен, и спросите снова.
workbench-problems-checking = Файл изменился, а языковой сервер ещё не проверил его заново: это проблемы прежнего текста; спросите снова.
workbench-problems-no-server = Файлы такого вида не проверяет ни языковой сервер, ни инспекция.
workbench-spelling-typo = Возможная опечатка: «{ $word }»
workbench-spelling-fix-typo = Исправить опечатку
workbench-spelling-rename-to = Опечатка: переименовать в «{ $name }»
workbench-spelling-save = Сохранить «{ $word }» в словарь
workbench-spelling-no-affixes = Рядом со словарём Hunspell нет файла .aff
workbench-spelling-bad-word = «{ $word }» — не слово: сохранить можно только английские буквы и апострофы
workbench-spelling-write-failed = Не удалось записать { $file }: { $error }
workbench-spelling-bad-file = Не удалось прочитать словарь { $file }: { $error }
workbench-spelling-no-application-dictionary = У этой IDE нет словаря уровня приложения (нет папки данных)
workbench-spelling-no-project = Проект не открыт, поэтому словаря проекта нет
workbench-spelling-not-checked = Орфография файла не проверена: инспекция выключена, файл не открыт или проверка не успела закончиться
workbench-spelling-no-typo = Здесь нет опечатки
workbench-spelling-not-changed = Опечатку нельзя заменить: файл только для чтения
workbench-spelling-too-large = Словарь больше { $megabytes } МБ
workbench-spelling-linked = { $folder } или его project.xml — символическая ссылка: слова записываются только в собственные файлы проекта
workbench-problems-power-save = Включён режим энергосбережения: файлы не проверяются по мере изменения. Выключите его: Файл → Режим энергосбережения.
workbench-language-server-disabled = { $server } выключен в настройках языковых серверов.
workbench-language-server-stopped = { $server } остановлен; перезапустите его, чтобы им пользоваться.
workbench-language-server-failed = { $server } не работает: { $reason }
workbench-lsp-install-manual = Установите { $server } сами и проверьте, что он есть в PATH.
workbench-lsp-tool-missing = { $tool ->
    [go] Go не установлен: сначала установите Go, затем { $server }.
    [npm] npm не установлен: сначала установите Node.js с npm, затем { $server }.
   *[other] Cargo не установлен: сначала установите Rust с Cargo, затем { $server }.
}
workbench-completion-none = Автодополнение не запущено.
workbench-fold-none = Здесь не начинается сворачиваемый блок.
workbench-fold-level = Уровень начинается с 1.
workbench-sticky-no-line = Строки { $line } нет.
workbench-nav-no-declaration = Переходить некуда: объявление не найдено
workbench-nav-no-implementation = Реализации не найдены
workbench-nav-no-type-declaration = Переходить некуда: объявление типа не найдено
workbench-nav-no-super-method = У метода нет супер-метода
workbench-nav-no-test = Тест не найден
workbench-nav-no-test-subject = Тестируемый код не найден
workbench-tests-unknown-language = Неизвестно, как называются тесты { $file }: ни языковой модуль, ни плагин этого не задают
workbench-tests-no-framework = Для этого файла не известна ни одна библиотека тестирования
workbench-tests-bad-name = «{ $name }» не может быть именем теста
workbench-tests-no-destination = Выберите папку для теста
workbench-tests-unknown-member = В { $file } нет { $member }
workbench-tests-outside-project = { $file } вне проекта: тест пишется внутри проекта
workbench-tests-cannot-write = Не удалось записать { $file }: { $error }
workbench-nav-no-usages = Использования в проекте не найдены
workbench-nav-usages-outdated = Файл изменился во время поиска; найдите использования ещё раз
workbench-show-usages-none = Нечего открыть: использования не показаны
workbench-nav-no-symbol = Под курсором нет символа
workbench-nav-no-file = Файл { $path } не найден
workbench-hierarchy-no-call = Нет функции или метода, для которых можно показать иерархию вызовов
workbench-hierarchy-no-type = Нет типа, для которого можно показать иерархию типов
workbench-hierarchy-unsupported-call = { $server } не строит иерархию вызовов
workbench-hierarchy-unsupported-type = { $server } не строит иерархию типов
workbench-hierarchy-direction = Иерархия вызовов показывает вызывающих или вызываемых, иерархия типов — супертипы или подтипы
workbench-hierarchy-not-open = Иерархия не открыта
workbench-hierarchy-no-node = В иерархии нет узла { $node }
workbench-hierarchy-no-tab = В окне «Иерархия» нет вкладки { $tab }
workbench-rename-not-here = Здесь нечего переименовать
workbench-rename-invalid-name = «{ $name }» не является допустимым идентификатором
workbench-rename-files-changed = Файлы изменились после расчёта переименования; ничего не изменено. Попробуйте ещё раз
workbench-rename-unsupported-change = Это переименование создаёт, перемещает или удаляет файлы, что не поддерживается; ничего не изменено
workbench-rename-overlapping = Изменения от языкового сервера пересекаются; ничего не изменено
workbench-safe-delete-title = Безопасное удаление
workbench-safe-delete-not-here = Здесь нечего удалять; поставьте курсор на имя объявления
workbench-safe-delete-unsupported = Языковой сервер этого файла не перечисляет его объявления, поэтому безопасное удаление не может их найти
workbench-safe-delete-usages-detected = Найдены использования
workbench-safe-delete-anyway = Всё равно удалить
workbench-safe-delete-view-usages = Показать использования
workbench-safe-delete-usage = Использование в { $file }:{ $line }
workbench-safe-delete-conflicts = { $count ->
    [one] У «{ $name }» { $count } использование, которое мешает безопасно удалить.
    [few] У «{ $name }» { $count } использования, которые мешают безопасно удалить.
   *[other] У «{ $name }» { $count } использований, которые мешают безопасно удалить.
}
workbench-safe-delete-file-conflicts = { $count ->
    [one] У объявлений { $name } { $count } использование, которое мешает безопасно удалить.
    [few] У объявлений { $name } { $count } использования, которые мешают безопасно удалить.
   *[other] У объявлений { $name } { $count } использований, которые мешают безопасно удалить.
}
workbench-safe-delete-unsaved = В { $file } есть несохранённые изменения: сохраните или отмените их, иначе «Безопасное удаление» их потеряет
workbench-safe-delete-has-usages = { $count ->
    [one] У «{ $name }» { $count } использование, поэтому ничего не удалено; передайте force, чтобы всё равно удалить
    [few] У «{ $name }» { $count } использования, поэтому ничего не удалено; передайте force, чтобы всё равно удалить
   *[other] У «{ $name }» { $count } использований, поэтому ничего не удалено; передайте force, чтобы всё равно удалить
}
workbench-rename-title = Переименование
workbench-rename-to = Переименовать «{ $name }» и его использования в:
workbench-refactoring-unknown = Рефакторинга «{ $id }» нет; их перечисляет refactor.dialog.list
workbench-refactoring-problems = Обнаружены проблемы
workbench-refactoring-continue = Продолжить
workbench-refactoring-view-conflicts = Показать конфликты
workbench-refactoring-conflicts = { $count ->
    [one] У рефакторинга { $count } конфликт.
    [few] У рефакторинга { $count } конфликта.
   *[other] У рефакторинга { $count } конфликтов.
}
workbench-refactoring-refused = { $conflicts } Ничего не изменено; передайте force, чтобы всё равно выполнить рефакторинг
workbench-java-move-title = Перемещение
workbench-java-move-class-title = Переместить класс { $name }
workbench-java-move-classes-title = { $count ->
    [one] Переместить { $count } класс
    [few] Переместить { $count } класса
   *[other] Переместить { $count } классов
}
workbench-java-move-package-title = Переместить пакет { $name }
workbench-java-move-to-package = В пакет:
workbench-java-move-not-here = Перемещение работает с классами и пакетами Java: выберите файлы .java или одну папку пакета
workbench-java-move-no-classes = В папке нет файлов Java, поэтому языковой сервер не может переместить её как пакет
workbench-java-move-no-destinations = Языковой сервер не предлагает пакетов для перемещения
workbench-java-move-choose = Выберите пакет из предложенных языковым сервером
workbench-java-move-same-package = Это уже в { $package }; выберите другой пакет
workbench-java-move-file-exists = В { $package } уже есть { $name }
workbench-java-move-package-exists = Пакет { $package } уже существует
workbench-java-move-refused = Языковой сервер не смог это переместить
workbench-java-move-package-private = { $name } не public, а { $file } использует его из старого пакета
workbench-java-signature-title = Изменение сигнатуры
workbench-java-signature-title-of = Изменить сигнатуру { $name }
workbench-java-signature-not-here = Поставьте курсор на имя метода или на его вызов, чтобы изменить сигнатуру
workbench-java-signature-visibility = Видимость:
workbench-java-signature-package-private = видимость пакета
workbench-java-signature-return-type = Тип результата:
workbench-java-signature-name = Имя
workbench-java-signature-parameters = Параметры:
workbench-java-signature-type = Тип
workbench-java-signature-default = Значение по умолчанию
workbench-java-signature-delegate = Оставить старый метод как делегат
workbench-java-signature-no-return-type = Введите тип результата
workbench-java-signature-no-type = Введите тип параметра «{ $name }»
workbench-java-signature-duplicate = Два параметра называются «{ $name }»
workbench-java-signature-default-needed = Введите значение по умолчанию нового параметра «{ $name }»: его передадут вызовы
workbench-java-signature-refused = Языковой сервер не смог изменить сигнатуру с этими значениями
workbench-java-signature-removed-used = Удалённый параметр «{ $name }» всё ещё используется в теле метода
workbench-java-hierarchy-not-here = Поставьте курсор в класс Java
workbench-java-hierarchy-no-type = Языковой сервер не находит здесь тип { $name }
workbench-java-hierarchy-members = Члены:
workbench-java-hierarchy-member = Член
workbench-java-hierarchy-make-abstract = Сделать абстрактным
workbench-java-hierarchy-choose-members = Выберите хотя бы один член
workbench-java-hierarchy-exists = В { $class } уже есть { $member }
workbench-java-hierarchy-uses-staying = { $name } остаётся в { $class }, но перенесённый член его использует
workbench-java-hierarchy-made-abstract = { $class } станет абстрактным, и код, который создаёт его экземпляры, перестанет компилироваться
workbench-java-hierarchy-file-exists = Класс { $name } там уже есть
workbench-java-pull-up-title = Перенести члены вверх
workbench-java-pull-up-title-of = Перенести члены вверх из { $name }
workbench-java-pull-up-to = Перенести члены в:
workbench-java-pull-up-no-super = У { $name } нет суперкласса или интерфейса в проекте, куда можно перенести члены
workbench-java-pull-up-interface-field = Поле { $name } не static final, поэтому интерфейс не может его содержать
workbench-java-pull-up-interface-private = Закрытый метод { $name } нельзя перенести в интерфейс
workbench-java-pull-up-interface-public = { $name } не public, а методы интерфейса открыты; он должен стать public
workbench-java-push-down-title = Перенести члены вниз
workbench-java-push-down-title-of = Перенести члены вниз из { $name }
workbench-java-push-down-to = Члены перейдут в { $classes }.
workbench-java-push-down-keep-abstract = Оставить абстрактным
workbench-java-push-down-class = Перенос членов вниз работает с классом
workbench-java-push-down-no-sub = У { $name } нет подклассов в проекте
workbench-java-push-down-private = { $name } закрыт в { $class }, поэтому подклассы не смогут его использовать
workbench-java-push-down-used = { $name } используется в { $file } в строке { $line }, где члены подклассов не видны
workbench-java-superclass-title = Выделить суперкласс
workbench-java-superclass-title-of = Выделить суперкласс из { $name }
workbench-java-superclass-name = Имя суперкласса:
workbench-java-superclass-class = Выделение суперкласса работает с классом
workbench-java-superclass-constructor = Этот вызов передаёт аргументы старому суперклассу; { $class } понадобится конструктор, который их принимает
workbench-java-interface-title = Выделить интерфейс
workbench-java-interface-title-of = Выделить интерфейс из { $name }
workbench-java-interface-name = Имя интерфейса:
workbench-java-interface-members = Члены, из которых составить интерфейс:
workbench-java-interface-refused = Языковой сервер не смог выделить интерфейс с этими значениями
workbench-java-encapsulate-title = Инкапсулировать поля
workbench-java-encapsulate-title-of = Инкапсулировать поля { $name }
workbench-java-encapsulate-fields = Поля для инкапсуляции:
workbench-java-encapsulate-field = Поле
workbench-java-encapsulate-getter = Геттер
workbench-java-encapsulate-setter = Сеттер
workbench-java-encapsulate-field-visibility = Видимость поля:
workbench-java-encapsulate-accessor-visibility = Видимость методов доступа:
workbench-java-encapsulate-none = В { $name } нет поля для инкапсуляции (объявление нескольких полей сразу не предлагается)
workbench-java-encapsulate-choose = Выберите хотя бы одно поле
workbench-java-encapsulate-no-field = { $name } больше не поле класса
workbench-java-encapsulate-no-accessor = Введите имя геттера или сеттера для { $name }
workbench-java-encapsulate-no-getter = { $name } здесь читается, но геттера у него не будет
workbench-java-encapsulate-no-setter = { $name } здесь записывается, но сеттера у него не будет
workbench-java-encapsulate-value = Присваивание { $name } используется как значение, поэтому его нельзя заменить вызовом сеттера
workbench-java-encapsulate-unsure = Это использование { $name } нельзя безопасно переписать; измените его вручную
workbench-java-encapsulate-import = { $name } здесь импортируется статически
workbench-java-parameter-object-title = Ввести объект параметров
workbench-java-parameter-object-title-of = Ввести объект параметров для { $name }
workbench-java-parameter-object-not-here = Поставьте курсор в метод или конструктор
workbench-java-parameter-object-no-parameters = У { $name } нет параметров
workbench-java-parameter-object-class-name = Имя класса:
workbench-java-parameter-object-kind = Создать:
workbench-java-parameter-object-class = Класс с геттерами
workbench-java-parameter-object-record = Запись (record)
workbench-java-parameter-object-parameter-name = Имя параметра:
workbench-java-parameter-object-parameter = Параметр
workbench-java-parameter-object-choose = Выберите хотя бы один параметр
workbench-java-parameter-object-varargs = Параметр переменной арности нельзя перенести в объект параметров
workbench-java-parameter-object-name-taken = Другой параметр уже называется { $name }
workbench-java-parameter-object-override = { $name } переопределяет метод; меняйте всю иерархию через «Изменить сигнатуру»
workbench-java-parameter-object-overridden = { $name } переопределён в { $file }; меняйте всю иерархию через «Изменить сигнатуру»
workbench-java-parameter-object-assigned = Параметру { $name } присваивается значение в теле, поэтому его нельзя заменить чтением из объекта
workbench-java-parameter-object-shadowed = В теле объявлен другой { $name }, и его использования не отличить
workbench-java-parameter-object-recursive = { $name } вызывает сам себя; измените его вручную или через «Изменить сигнатуру»
workbench-java-parameter-object-reference = Здесь { $name } используется как ссылка на метод, её нельзя переписать
workbench-java-parameter-object-arguments = Этот вызов { $name } передаёт другое число аргументов
workbench-undo-skipped = { $count ->
    [one] Не отменено в { $count } файле, изменённом после этого: { $files }
    [few] Не отменено в { $count } файлах, изменённых после этого: { $files }
   *[other] Не отменено в { $count } файлах, изменённых после этого: { $files }
}
workbench-format-failed = { $file } сохранён без форматирования: { $error }
workbench-format-timeout = { $file } сохранён без форматирования: языковой сервер не ответил вовремя.
workbench-format-outdated = Файл изменился, пока языковой сервер его форматировал; ничего не применено.
workbench-format-bad-edits = Языковой сервер прислал пересекающиеся правки; ничего не применено.
workbench-format-sql-stopped = Форматирование SQL остановилось без ответа; ничего не применено.
workbench-format-sql-kept = { $count ->
    [one] { $count } запрос оставлен как есть: в нём не закрыта кавычка или комментарий либо не парные скобки.
    [few] { $count } запроса оставлены как есть: в них не закрыты кавычки или комментарии либо не парные скобки.
   *[other] { $count } запросов оставлены как есть: в них не закрыты кавычки или комментарии либо не парные скобки.
}
workbench-format-sql-timeout = { $file } сохранён без форматирования: форматирование SQL заняло слишком много времени.
workbench-format-sql-too-long = SQL длиннее 1 МиБ; отформатируйте его в файле.
workbench-format-sql-no-connection = Нет подключения «{ $id }», чтобы взять из него диалект.

# Темы (ui.theme.*).
workbench-theme-error-unknown = Темы «{ $id }» нет.
workbench-theme-error-broken = Тему «{ $id }» нельзя использовать: { $error }
workbench-theme-error-no-folder = Нет папки тем, куда можно импортировать.
workbench-theme-error-read = Не удалось прочитать { $path }: { $error }
workbench-theme-error-format = { $path } не является темой известного формата: темой Zen, цветовой схемой или темой интерфейса JetBrains, цветовой темой VS Code.
workbench-theme-error-import = Не удалось импортировать { $path }: { $error }
workbench-theme-error-write = Не удалось сохранить тему в { $path }: { $error }
workbench-theme-missing = Тема «{ $id }» не найдена; вместо неё используется встроенная тема.
workbench-fix-none = Укажите, какие быстрые исправления применить
workbench-fix-unknown = Сейчас в этом файле нет быстрого исправления { $id }
workbench-fix-failed = Не удалось выполнить «{ $fix }»

workbench-templates-none = Для этого файла нет живого шаблона «{ $name }».
workbench-surround-unknown = В этом файле нечем обернуть код по имени «{ $name }».
workbench-surround-description = Описание
workbench-surround-region = Комментарии region…endregion
workbench-surround-editor-fold = Комментарии <editor-fold…>
workbench-templates-invalid-language = У живого шаблона должен быть язык.
workbench-templates-invalid-abbreviation = Аббревиатура не может быть пустой или содержать пробелы.
workbench-templates-invalid-text = У живого шаблона должен быть текст.
workbench-templates-invalid-variable = «{ $name }» — не имя переменной: допустимы буквы, цифры и _.
workbench-templates-builtin = Живой шаблон «{ $name }» встроенный: его можно выключить или восстановить.
workbench-templates-unknown = Для { $language } нет живого шаблона «{ $name }».
workbench-templates-exists = Для { $language } уже есть живой шаблон «{ $name }».
workbench-templates-save-failed = Не удаётся сохранить живые шаблоны: { $detail }
workbench-templates-invalid-import = «{ $name }» — не пакет для импорта.
workbench-templates-unreadable = Не удаётся прочитать живые шаблоны, используются встроенные: { $detail }
workbench-templates-invalid-expression = Выражение переменной «{ $name }» не читается: { $detail }
workbench-templates-expression-syntax = напишите текст в кавычках, имя переменной или функцию, например capitalize(NAME)
workbench-templates-expression-quote = текст в кавычках не закрыт
workbench-templates-expression-function = функции { $name }() нет
workbench-templates-plugin = Живой шаблон «{ $name }» пришёл из плагина: выключите его или продублируйте, чтобы изменить.
workbench-postfix-unknown = Постфиксного шаблона «{ $name }» для { $language } нет.
workbench-postfix-none = Здесь после выражения нет постфиксного шаблона «{ $name }».
workbench-templates-corrupt = Файл живых шаблонов повреждён ({ $detail }); используются встроенные шаблоны, а файл сохранён как { $file }.
workbench-templates-skipped = { $count ->
    [one] { $count } живой шаблон в templates.json неверен и пропущен.
    [few] { $count } живых шаблона в templates.json неверны и пропущены.
   *[other] { $count } живых шаблонов в templates.json неверны и пропущены.
}

# Context actions (editor.intentions.*).
workbench-extract-variable = Выделить переменную
workbench-extract-constant = Выделить константу
workbench-extract-field = Выделить поле
workbench-extract-parameter = Выделить параметр
workbench-extract-method = Выделить метод
workbench-extract-inline = Встроить
workbench-extract-none = «{ $refactoring }» здесь недоступно
workbench-extract-no-variant = Способа номер { $index } нет; запросите снова без варианта
workbench-extract-unsupported = Языковой сервер этого файла не предлагает рефакторингов
workbench-intentions-unasked-edit = IDE не запрашивала это изменение; оно не применено
workbench-intentions-unknown = Контекстного действия «{ $id }» нет; запросите список действий заново
workbench-intentions-stale = Файл изменился после того, как были получены контекстные действия; запросите их заново
workbench-generate-none = Здесь нечего сгенерировать
workbench-generate-unknown = Генератора «{ $id }» нет; запросите список генераторов заново
workbench-generate-stale = Файл изменился после того, как был получен список генераторов; запросите его заново
workbench-generate-no-chooser = Этот генератор не предлагает выбрать члены класса
workbench-generate-unknown-member = Члена «{ $id }» нет; запросите список членов заново
workbench-generate-nothing-chosen = Выберите хотя бы один член
workbench-intentions-nothing = Этому действию нечего применять
workbench-intentions-files-changed = Файлы изменились после расчёта действия; ничего не изменено. Попробуйте ещё раз
workbench-intentions-unsupported-change = Это действие создаёт, перемещает или удаляет файлы, что не поддерживается; ничего не изменено
workbench-edit-outside-project = Изменение затрагивает { $path } вне проекта; ничего не изменено
workbench-edit-file-exists = { $path } уже существует; ничего не изменено
workbench-edit-file-missing = { $path } не существует; ничего не изменено
workbench-edit-folder-not-empty = Папка { $path } не пуста; ничего не изменено
workbench-edit-delete-too-large = { $path } слишком велик, чтобы удалить его с возможностью отмены; ничего не изменено
workbench-file-operation-timeout = Языковой сервер { $server } не ответил вовремя; ссылки на файл не обновлены
workbench-file-operation-edit-failed = Изменения языкового сервера { $server } не применены: { $error }
workbench-file-operation-missed = Языковой сервер { $server } мог не обновить { $count ->
    [one] { $count } файл, где
    [few] { $count } файла, где
   *[many] { $count } файлов, где
} ещё упоминается { $name }: { $files }

# Notifications (ui.notifications.*).
workbench-notification-unknown = Уведомления { $id } нет.
workbench-notification-no-action = У уведомления { $id } нет действия «{ $action }».
workbench-notification-empty = У уведомления должен быть заголовок или текст.
workbench-task-none = Такой фоновой задачи нет
workbench-task-cancelled = Отменено
workbench-task-cancelling = Отмена…
workbench-language-task = { $server }: { $title }
workbench-fix-task-one = { $fix }
workbench-fix-task-many = Применение быстрых исправлений: { $count }
workbench-fix-done = изменённые файлы прочитаны заново
workbench-fix-details = { $id } показывает подробности проблемы: их можно прочитать в самой проблеме (problems.file.list, details)
workbench-fix-action = { $id } — действие окна: выполните { $action } через run_action
workbench-error-compare-folder = { $path } — папка; сравнивать можно файлы.
workbench-compare-editor = { $file } (редактор)
workbench-compare-clipboard = Буфер обмена
workbench-compare-selection = { $file } (выделение)
workbench-error-clipboard-empty = В буфере обмена нет текста для сравнения.
workbench-error-compare-read-only = { $file } нельзя изменить здесь.
workbench-error-compare-not-folder = { $path } — не папка; «Сравнить папки» сравнивает две папки.
workbench-error-sync-outside = { $path } — не путь внутри сравниваемых папок.
workbench-error-sync-missing = { $path } нет, его нельзя скопировать или удалить.
workbench-error-sync-delete-both = { $path } есть в обеих папках; скопируйте его на одну сторону, а не удаляйте.
workbench-error-reveal = Не удалось открыть файловый менеджер: { $error }
workbench-index-title = Индексация…
workbench-index-progress = { $done } из { $total } файлов
workbench-index-done = { $files ->
    [one] Проиндексирован { $files } файл
    [few] Проиндексировано { $files } файла
   *[other] Проиндексировано { $files } файлов
}
workbench-replace-pair = Укажите oldText и newText вместе или edits.
workbench-replace-nothing = Заменять нечего: укажите oldText и newText или edits.
workbench-replace-empty = oldText пуст: укажите точный текст для замены.
workbench-replace-not-found = Текста для замены нет в { $file }; возможно, он изменился: прочитайте строки заново (get_document_text).
workbench-replace-not-unique = Текст для замены встречается в { $file } { $count } раз(а), а не { $expected }: дайте больше текста вокруг или expectedCount.
workbench-replace-overlap = Две правки меняют один и тот же текст { $file }; дайте каждой правке свой текст.
workbench-replace-read-only = { $file } нельзя изменить: { $reason }
workbench-replace-refused = { $file } нельзя изменить.
workbench-replace-bad-regex = oldText — не правильное регулярное выражение: { $error }

workbench-error-launcher-no-home = Домашняя папка неизвестна, поэтому нет ни папки по умолчанию, ни профиля оболочки.
workbench-error-launcher-relative = Папка скрипта запуска должна быть абсолютным путём или начинаться с ~, а не «{ $path }».
workbench-error-launcher-name = «{ $name }» не может быть именем скрипта запуска: используйте буквы, цифры, «-», «_» и «.».
workbench-error-launcher-executable = Исполняемый файл этой IDE неизвестен, поэтому скрипту запуска нечего запускать.
workbench-error-launcher-foreign = { $path } создан не Zen и остаётся как есть: выберите другое имя или папку.
workbench-error-launcher-write = Не удалось записать скрипт запуска { $path }: { $error }
workbench-error-launcher-profile = Не удалось добавить строку в профиль оболочки: { $error }

workbench-scratch-plain-text = Обычный текст
workbench-scratch-buffers = Буферы
workbench-scratch-no-folder = У IDE нет папки данных для черновиков.
workbench-scratch-no-language = Для черновиков нет языка «{ $language }».
workbench-scratch-not-scratch = { $path } — не черновик.
workbench-scratch-from-selection = Создать черновик из выделенного
workbench-file-templates-unclosed-comment = Комментарий «#*» не закрыт «*#».
workbench-file-templates-unclosed-raw = Текст «#[[» не закрыт «]]#».
workbench-file-templates-unclosed-string = Строка не закрыта.
workbench-file-templates-unclosed-if = «#if» не закрыт «#end».
workbench-file-templates-else-after-else = «#else» или «#elseif» после «#else».
workbench-file-templates-expected = Здесь ожидается «{ $what }».
workbench-file-templates-stray = «#{ $directive }» без «#if».
workbench-file-templates-unsupported = «#{ $directive }» в шаблонах файлов не поддерживается.
workbench-file-templates-unsupported-reference = Методы, свойства и другие выражения в ссылке не поддерживаются: используйте переменную, например «${"{"}NAME{"}"}».
workbench-file-templates-unsupported-expression = Присвоить можно только строку, число, true, false или переменную: выражения не поддерживаются.
workbench-file-templates-bad-set = «#set» нужна переменная, например «#set($NAME = "value")».
workbench-file-templates-bad-value = «#set» нужно значение: строка, число, true, false или переменная.
workbench-file-templates-bad-parse = «#parse» нужно имя включаемого шаблона, например «#parse("File Header.java")».
workbench-file-templates-bad-condition = Условие не читается: используйте переменные, строки, «==», «!=», «!», «&&» и «||».
workbench-file-templates-parse-deep = Включаемые шаблоны подключают друг друга слишком глубоко.
workbench-file-templates-parse-many = Шаблон подключает включаемые шаблоны больше { $count } раз.
workbench-file-templates-nested-deep = «#if» и условия вложены слишком глубоко.
workbench-file-templates-too-long = Готовый текст длиннее { $size } КБ.
workbench-file-templates-linked = «{ $path }» — символическая ссылка: шаблоны файлов через неё не записываются.
workbench-file-templates-no-include = Нет включаемого шаблона «{ $name }».
workbench-file-templates-invalid-name = «{ $name }» не может быть именем шаблона.
workbench-file-templates-invalid-extension = «{ $extension }» не может быть расширением: укажите его без точки и пробелов.
workbench-file-templates-invalid-file-name = Шаблон имени файла — одна строка.
workbench-file-templates-bad-file-name = Шаблон имени файла дал «{ $name }»: нужно имя файла в выбранной папке или в папке внутри неё, без «..» и не абсолютный путь.
workbench-file-templates-builtin = Шаблон файла «{ $name }» встроенный: его можно только сбросить.
workbench-file-templates-unknown = Нет шаблона файла «{ $name }».
workbench-file-templates-exists = Шаблон файла «{ $name }» уже есть.
workbench-file-templates-save-failed = Не удалось сохранить шаблоны файлов: { $detail }
workbench-file-templates-missing = { $count ->
    [one] У переменной { $names } нет значения.
    [few] У переменных { $names } нет значений.
   *[other] У переменных { $names } нет значений.
}
workbench-file-templates-cannot-render = Шаблон файла «{ $name }» не удаётся применить: { $problems }
workbench-file-templates-var-name = Имя нового файла, как его ввели, без расширения
workbench-file-templates-var-file-name = Имя нового файла с расширением
workbench-file-templates-var-dir-path = Папка нового файла относительно проекта
workbench-file-templates-var-package-name = Пакет нового файла: его папки ниже корня исходников через точку (com.acme)
workbench-file-templates-var-project-name = Имя проекта
workbench-file-templates-var-product-name = Имя IDE
workbench-file-templates-var-user = Имя текущего пользователя в системе
workbench-file-templates-var-date = Сегодняшняя дата, например 2026-10-22
workbench-file-templates-var-time = Время, например 14:05
workbench-file-templates-var-year = Год
workbench-file-templates-var-month = Месяц, от 01 до 12
workbench-file-templates-var-month-name-short = Название месяца, первые три буквы (Oct)
workbench-file-templates-var-month-name-full = Название месяца (October)
workbench-file-templates-var-day = День месяца, от 01 до 31
workbench-file-templates-var-day-name-short = Название дня недели, первые три буквы (Thu)
workbench-file-templates-var-day-name-full = Название дня недели (Thursday)
workbench-file-templates-var-hour = Час, от 00 до 23
workbench-file-templates-var-minute = Минута
workbench-file-templates-var-second = Секунда
workbench-file-templates-var-ds = Знак доллара, «$»
file-type-text = Текст
file-type-detected = Тип файла определяется по содержимому
file-type-native = Файлы, открываемые в связанных приложениях
workbench-error-unknown-file-type = Файл { $file } не связан ни с одним типом файлов: выберите, как открывать такие файлы (editor.fileTypes.associate).
workbench-error-file-type-override = Отдельному файлу нельзя назначить тип «по содержимому» или открытие в приложении: выберите другой тип.
workbench-error-override-folder = { $path } — папка: тип назначается только файлам.
workbench-error-open-application = Не удалось открыть { $file } в его приложении: { $error }
workbench-error-not-a-regular-file = { $file } — не обычный файл (папка, канал, сокет или устройство), поэтому он не открывается.
workbench-error-opens-in-application = { $file } открывается в связанном с ним приложении: откройте его сами, чтобы запустить это приложение.
workbench-error-program-in-application = { $file } — исполняемая программа (или ссылка на неё), поэтому IDE не запускает её в приложении: запустите её сами, если доверяете ей.
workbench-opened-in-application = Файл { $file } открыт в его приложении.
workbench-binary-read-only = { $file } — не текстовый файл, поэтому он открыт только для чтения.
workbench-error-file-type-unknown = Типа файлов «{ $id }» нет.
workbench-error-file-type-pattern = «{ $pattern }» — не шаблон имени файла: он пуст или содержит путь к папке.

workbench-editorconfig-problem = Строка { $line }: { $message }
workbench-editorconfig-not-a-property = «{ $text }» — не секция, не свойство и не комментарий.
workbench-editorconfig-unclosed-section = У секции «{ $text }» нет закрывающей «]».
workbench-editorconfig-outside-section = { $name } стоит до первой секции, где читается только root.
workbench-editorconfig-no-name = У свойства нет имени перед «=».
workbench-editorconfig-invalid-value = { $name } не может быть «{ $value }».
workbench-editorconfig-outside = { $path } — не файл открытого проекта.
workbench-editorconfig-none = Ни в одном файле .editorconfig проекта нет секции для { $path }.
workbench-editorconfig-exists = { $path } уже существует.
workbench-error-unmappable = Символ { $character } ({ $code }) в строке { $line }, столбце { $column } нельзя записать в { $charset } — кодировке файла, поэтому файл не сохранён: на диске в нём ничего не изменилось. Удалите или замените символ либо измените кодировку файла (в строке состояния или в charset файла .editorconfig, если он её задаёт).
workbench-indent-detection-off = Выключено «Следовать отступам, которые уже есть в файле» (Настройки → Редактор → Стиль кода).
workbench-indent-nothing-detected = Собственный отступ файла не определён или совпадает с настроенным.
workbench-unwrap-unwrap = Развернуть '{ $construct }'
workbench-unwrap-remove = Удалить '{ $construct }'
workbench-unwrap-braces = Развернуть фигурные скобки
workbench-unwrap-no-entry = Здесь у «Развернуть/удалить» нет пункта { $index }; их { $count } (с 0, от внутреннего).

workbench-todo-no-file = В редакторе не открыт файл.
workbench-error-replace-encoding = { $file } — не текст в { $charset }, который Zen IDE умеет править; ничего не заменено.
workbench-error-replace-read-only = { $file } только для чтения; ничего не заменено.
workbench-error-replace-changed = { $file } изменился после поиска; ничего не заменено. Повторите поиск.
workbench-error-replace-not-restored = { $file } не удалось вернуть в прежний вид: { $error }
workbench-results-none = В окне Find нет вкладки с вхождениями.
workbench-results-title = Вхождения '{ $text }'
workbench-results-target-replace = Замена '{ $text }' на '{ $replacement }'
workbench-results-in-project = Проект
workbench-results-in-directory = Папка '{ $directory }'
workbench-results-task = Поиск '{ $text }'
workbench-results-task-in = в { $file }…
workbench-results-task-done = { $count ->
    [one] Найдено { $count } вхождение
    [few] Найдено { $count } вхождения
   *[other] Найдено { $count } вхождений
}
workbench-results-task-stopped = { $count ->
    [one] Поиск остановлен: найдено { $count } вхождение
    [few] Поиск остановлен: найдено { $count } вхождения
   *[other] Поиск остановлен: найдено { $count } вхождений
}
workbench-results-not-asking = «Слишком много результатов» не спрашивает об этом поиске.
workbench-results-targets = Цели
workbench-results-found = Найденные вхождения — { $scope }
workbench-results-unclassified = Без категории
workbench-results-in-comments = В комментариях
workbench-results-in-strings = В строковых константах
workbench-results-production = Рабочий код
workbench-results-test = Тесты
workbench-results-no-node = Сначала выберите узел или назовите его.
workbench-results-no-such-node = Узла { $node } на этой вкладке нет; прочитайте search.results.tree заново.
workbench-results-no-usages = На этой вкладке нет вхождений.
workbench-results-usages-title = Использования { $symbol }
workbench-results-numbered = { $title } ({ $number })
workbench-results-usages-target = { $kind } { $symbol }
workbench-results-found-usages = Использования — { $scope }
workbench-results-read = Чтение значения
workbench-results-write = Запись значения
workbench-results-import = В импорте
workbench-usages-scope-current-file = Текущий файл
workbench-usages-scope-open-files = Открытые файлы
workbench-usages-scope-project-files = Файлы проекта
workbench-usages-scope-all-places = Все места
workbench-usages-kind-variable = Переменная
workbench-usages-kind-parameter = Параметр
workbench-usages-kind-constant = Константа
workbench-usages-kind-function = Функция
workbench-usages-kind-method = Метод
workbench-usages-kind-type = Тип
workbench-usages-kind-struct = Структура
workbench-usages-kind-interface = Интерфейс
workbench-usages-kind-class = Класс
workbench-usages-kind-field = Поле
workbench-usages-kind-property = Свойство
workbench-usages-kind-package = Пакет
workbench-usages-kind-module = Модуль
workbench-usages-kind-symbol = Символ
workbench-usages-task = Поиск использований '{ $symbol }'
workbench-usages-task-done = { $count ->
    [one] Найдено { $count } использование
    [few] Найдено { $count } использования
   *[other] Найдено { $count } использований
}
workbench-usages-task-replaced = Заменён новым поиском использований
workbench-usages-task-stopped = Поиск использований остановлен
workbench-usages-none = Использования не найдены — { $scope }
workbench-usages-no-recent = Такого поиска нет среди недавних поисков использований.
workbench-usages-recent-gone = { $symbol } больше нет в этом файле.
workbench-usages-settings-closed = Диалог настроек поиска использований не открыт.
workbench-results-not-replace = На этой вкладке вхождения поиска, а не замены: вкладки с заменой открывает Replace in Files (⇧⌘R).
workbench-results-nothing-to-replace = Заменять нечего: все выбранные вхождения исключены.
workbench-results-replace-running = Дождитесь конца поиска или остановите его, прежде чем заменять.
workbench-node-kind = Node.js
workbench-node-test-kind = Тесты Node.js
workbench-node-field-file = Файл JavaScript:
workbench-node-field-files = Файлы тестов:
workbench-node-field-test-name = Имя теста:
workbench-node-field-runner = Средство запуска тестов:
workbench-node-field-options = Параметры Node:
workbench-node-field-interpreter = Интерпретатор Node:
workbench-node-no-file = укажите файл для запуска
workbench-node-not-found = { $program } не найден на этом компьютере; установите Node.js (https://nodejs.org) и попробуйте снова
workbench-node-unknown-kind = «{ $kind }» — не конфигурация Node.js
workbench-node-unknown-runner = «{ $runner }» — неизвестное IDE средство запуска тестов: нужен jest или vitest
workbench-node-no-runner = { $runner } нет в node_modules; сначала установите зависимости проекта
workbench-npm-kind = npm
workbench-npm-field-package-json = package.json:
workbench-npm-field-command = Команда:
workbench-npm-field-script = Скрипты:
workbench-npm-field-package-manager = Менеджер пакетов:
workbench-npm-unknown-command = Конфигурации npm выполняют команды run, exec и install, а не «{ $command }».
workbench-npm-unknown-manager = «{ $manager }» — неизвестный IDE менеджер пакетов: нужен npm, pnpm, yarn или bun.
workbench-npm-no-script = укажите скрипт или программу для запуска
workbench-npm-not-found = { $program } не найден на этом компьютере; установите Node.js (https://nodejs.org) или менеджер пакетов и попробуйте снова
workbench-npm-offline-unsupported = У { $manager } нет автономного режима: чтобы установить пакеты, выключите автономный режим в окне npm
workbench-node-debug-not-running = Программа не запущена.
workbench-node-debug-no-answer = Node.js не ответил на { $method }.
workbench-node-debug-unsupported = { $request } не поддерживается для Node.js.
workbench-node-debug-no-program = Нет Node.js для запуска.
workbench-node-debug-not-started = Node.js не запустился.
workbench-node-debug-ended = Node.js завершился с кодом { $code } до подключения отладчика.
workbench-node-debug-no-source = У точек останова нет исходного файла.
workbench-node-debug-moved-on = Программа уже выполняется дальше.
workbench-node-debug-expression-failed = Выражение не вычислилось.
workbench-node-debug-filter-all = Все исключения
workbench-node-debug-filter-uncaught = Неперехваченные исключения
workbench-node-debug-too-large = Отладчик прислал слишком большое сообщение.
workbench-node-debug-not-text = Отладчик прислал не текст.
workbench-node-debug-not-ws = Адрес отладчика не начинается с ws://.
workbench-node-debug-not-local = Отладчик не на этом компьютере: { $host }.
workbench-node-debug-closed = Отладчик закрыл соединение.
workbench-npm-settings-outside = { $path } ведёт за пределы проекта, поэтому запись не выполнена.
workbench-npm-online-person = ИИ-агент не может вернуть проект в сеть: выключить автономный режим можете только вы.
workbench-npm-not-listed = «{ $path }» — не package.json этого проекта; список даёт npm.project.get
workbench-npm-no-such-script = В { $path } нет скрипта «{ $script }»
workbench-plugin-project-unsupported = Плагин «{ $plugin }» не установлен или не поддерживает этот проект.
workbench-plugin-project-no-setup = У плагина { $plugin } нет шага подготовки проекта.
workbench-plugin-project-no-setups = Ни у одного установленного плагина нет шага подготовки для этого проекта.
workbench-preview-none = У { $file } нет предпросмотра: он есть у файлов SVG и Markdown.
workbench-preview-too-large = Файл больше { $size } МБ, слишком велик для предпросмотра.
workbench-markdown-read = Не удалось прочитать { $path }: { $error }
workbench-markdown-format = «{ $format }» — не формат предпросмотра: нужен text, html или blocks.
workbench-markdown-no-task = В строке { $line } нет флажка задачи.
workbench-markdown-not-markdown = Активный файл — не Markdown.
workbench-markdown-heading-level = Уровня заголовка { $level } нет: укажите от 1 до 6 или 0 для обычного текста.
workbench-markdown-mermaid-too-deep = В диаграмме больше { $depth } уровней вложенности, поэтому она не рисуется.
workbench-markdown-mermaid-too-large = Диаграмма больше { $size } КиБ.
workbench-markdown-mermaid-crashed = Отрисовщик не смог прочитать эту диаграмму.
workbench-markdown-host = «{ $host }» — не имя хоста.
workbench-usages-no-declaration = Языковые серверы не знают объявления с именем { $symbol }; navigate.symbol.goTo показывает, что они знают.
workbench-usages-no-name = В { $path } нет { $name } в этом месте как целого слова.
workbench-hidden-change = { $file } скрыт от агентов: агент его не читает и не меняет (Настройки → Инструменты → MCP-сервер → Файлы, скрытые от агентов).
workbench-file-templates-children-files-only = Дочерние шаблоны бывают только у шаблонов вкладки Files.
workbench-new-file = Файл
workbench-new-directory = Папку
workbench-new-package = Пакет
workbench-ide-settings-error-no-data = У этой IDE нет папки данных, поэтому нет и своих настроек для экспорта, импорта или сброса.
workbench-ide-settings-error-relative = «{ $path }» — не абсолютный путь.
workbench-ide-settings-error-project = Настройки никогда не записываются в папку настроек проекта (.zen, .idea): { $path }
workbench-ide-settings-error-folder = { $path } — папка: укажите путь к ZIP-файлу, который нужно записать.
workbench-ide-settings-error-no-components = Выберите хотя бы один компонент.
workbench-ide-settings-error-nothing = В { $path } нечего импортировать из выбранных компонентов.
workbench-ide-settings-error-read = Не удалось прочитать { $path }: { $error }
workbench-ide-settings-error-write = Не удалось записать { $path }: { $error }
workbench-ide-settings-error-backup = Не удалось сохранить резервную копию настроек в { $path }: { $error }. Ничего не изменено.
workbench-ide-settings-error-unknown = В { $path } нет настроек, которые Zen IDE умеет читать: это не экспорт или резервная копия Zen IDE, не ZIP из Export Settings JetBrains и не файл раскладки или темы.
workbench-ide-settings-error-not-zip = { $path } — не ZIP-архив и не папка.
workbench-ide-settings-error-zip64 = { $path } — архив ZIP64, такие не читаются: архивы настроек небольшие.
workbench-ide-settings-error-too-large = { $path } отклонён: он больше { $megabytes } МБ.
workbench-ide-settings-error-too-many = { $path } отклонён: в нём больше { $count } записей.
workbench-ide-settings-error-entry-too-large = { $path } отклонён: «{ $entry }» распаковывается больше чем в { $megabytes } МБ.
workbench-ide-settings-error-unsafe = { $path } отклонён: запись «{ $entry }» была бы записана за пределы папки, в которую распаковывается.
workbench-ide-settings-error-damaged = { $path } повреждён: { $error }
workbench-ide-settings-skip-unknown = это не настройка, которую хранит Zen IDE
workbench-ide-settings-skip-jetbrains = настройка JetBrains, которую Zen IDE не читает; импортируются только раскладки клавиатуры и цветовые схемы
workbench-ide-settings-skip-encrypted = зашифрован
workbench-ide-settings-skip-method = упакован методом, который Zen IDE не читает ({ $method })
workbench-ide-settings-skip-damaged = не читается: { $error }
workbench-ide-settings-skip-invalid = некорректен: { $error }
workbench-ide-settings-skip-link = символическая ссылка
workbench-ide-settings-skip-too-large = больше { $megabytes } МБ
workbench-ide-settings-skip-write = не удалось записать: { $error }
workbench-ide-settings-skip-plugin = плагин { $id } { $version } здесь не установлен: установите его в Настройки → Плагины

# Конфигурации запуска Java (ADR-0152).
workbench-java-kind-application = Приложение
workbench-java-kind-jar = JAR-приложение
workbench-java-kind-file = Программа Java из одного файла
workbench-java-field-main-class = Главный класс:
workbench-java-field-module = Модуль:
workbench-java-field-jre = JRE:
workbench-java-field-build = Собрать перед запуском
workbench-java-field-coroutine-debug = Режим отладки корутин
workbench-java-field-coroutine-debug-default = Как в настройках
workbench-java-field-coroutine-debug-on = Включён при отладке
workbench-java-field-coroutine-debug-off = Выключен
workbench-java-field-track-coroutines = Отслеживать корутины (kotlinx-coroutines-debug из проекта)
workbench-java-field-jar = Путь к JAR:
workbench-java-field-file = Исходный файл:
workbench-java-field-classpath = Использовать classpath модуля
workbench-java-no-main-class = укажите главный класс, например shop.App
workbench-java-no-jar = укажите JAR для запуска
workbench-java-no-file = укажите исходный файл Java для запуска
workbench-java-no-jdk = у проекта нет JDK: выберите его в Настройки → Языки и фреймворки → JDK
workbench-java-no-java = в { $path } нет bin/java; выберите другую JRE
workbench-java-no-sources = в { $path } нет исходников Java
workbench-java-tool-missing = { $program } не найден на этом компьютере, и в проекте нет обёртки; установите его и повторите
workbench-java-unknown-kind = «{ $kind }» — не конфигурация Java
workbench-java-kotlin-built-by = В модуле есть исходники на Kotlin, поэтому его собирает { $tool }: компилятор IDE собирает только Java
workbench-java-kotlin-needs-build = код на Kotlin собирают Maven или Gradle: добавьте pom.xml или build.gradle(.kts) с плагином Kotlin
workbench-kotlin-kind-file = Программа Kotlin из одного файла
workbench-kotlin-field-compiler = Компилятор Kotlin (kotlinc):
workbench-kotlin-no-file = укажите исходный файл Kotlin для запуска
workbench-kotlin-no-compiler = для запуска файла Kotlin нужен компилятор Kotlin, а IDE его не поставляет и не скачивает: установите kotlinc (Homebrew, SDKMAN или архив выпуска), чтобы он был в PATH или в KOTLIN_HOME, или укажите его в конфигурации
workbench-console-jshell = JShell
workbench-console-kotlin = Kotlin REPL
workbench-console-no-jshell = в { $path } нет bin/jshell: JShell есть в JDK 9 и новее; выберите для проекта другой JDK
workbench-console-no-kotlinc = для Kotlin REPL нужен компилятор Kotlin, а IDE его не поставляет и не скачивает: установите kotlinc (Homebrew, SDKMAN или архив выпуска), чтобы он был в PATH или в KOTLIN_HOME
workbench-console-no-module = в проекте нет папки «{ $module }»
workbench-console-cannot-start = { $name } не запускается: { $error }
workbench-console-preparing = { $name }: собирается classpath модуля
workbench-console-failed = сборка classpath неожиданно прервалась
workbench-console-not-running = { $name } не запущен
workbench-bytecode-read-only = { $file } показывает скомпилированный байт-код, его нельзя редактировать
workbench-bytecode-no-javap = в { $path } нет bin/javap; выберите для проекта полный JDK
workbench-bytecode-failed = не удалось показать байт-код { $class }: { $error }
workbench-bytecode-no-file = откройте файл Java или Kotlin, чтобы увидеть его байт-код
workbench-bytecode-not-java = { $path } — не исходный файл Java и не Kotlin
workbench-bytecode-kotlin-alone = { $path } — файл Kotlin вне Maven и Gradle: его классы показываются, когда модуль Maven или Gradle его собирает
workbench-bytecode-no-class = в этом месте { $path } нет класса
workbench-bytecode-not-compiled = { $class } ещё не скомпилирован: соберите модуль, чтобы увидеть его байт-код
workbench-bytecode-build = Собрать
workspace-error-unknown = Рабочей области «{ $id }» нет.
workspace-error-no-tab = В рабочей области нет вкладки «{ $tab }».
workspace-error-not-closable = Вкладку «{ $title }» закрыть нельзя.
workspace-error-tab-kind = Вкладке без файла нужны вид и заголовок.
workspace-error-not-hosted = { $name } открывается в редакторе, а не в этом рабочем пространстве.
workspace-error-not-text = { $command } работает с текстовым файлом, а впереди вкладка «{ $tab }» рабочей области «{ $workspace }»; выберите консоль (workspace.tab.select) или сверните рабочую область (workspace.host.collapse).
workspace-error-move-between = Вкладку нельзя перенести из одной рабочей области в другую.
workspace-error-console-stays = Консоль «{ $file }» остаётся в своей рабочей области: консоли хранятся в папке данных IDE, а не в проекте. Сохраните её текст как скрипт проекта, чтобы открыть его в редакторе.
workspace-error-not-taken = Рабочая область «{ $workspace }» не принимает «{ $file }»: между ней и редактором переносятся только файлы проекта, с которыми она работает.
workspace-error-tab-stays = Вкладку «{ $tab }» рисует её рабочая область, и она остаётся там.
workspace-error-split-alone = Вкладка «{ $tab }» — единственная в своей группе; в ней ничего не останется.
workspace-tab-hidden-from-agent = Эта вкладка относится к подключению к базе данных, закрытому для агентов.
workbench-error-move-where = Укажите группу, в которую переносится вкладка (`to`), или рабочую область (`workspace`).
workbench-test-history-unknown = В истории нет запуска тестов { $id }
workbench-trust-prompt-title = Доверять проекту { $project }?
workbench-trust-prompt-body = Его сборка может выполнять код на этом компьютере: пока вы не доверяете проекту, IDE сама не запускает его сценарии сборки, не импортирует его через Gradle, запускает языковой сервер Java без Lombok и процессоров аннотаций и не применяет окружение Go из .zen/go.json. Доверяйте, только если доверяете его источнику.
workbench-trust-action = Доверять проекту
workbench-trust-agent = Доверять проекту может только человек, агенты — нет.
workbench-trust-agent-build = Проект { $project } не доверенный: агентам нельзя запускать его сборку. Человек может доверять ему командой «Доверять проекту».
workbench-trust-no-project = Проект не открыт.
workbench-trust-no-data = У IDE нет папки данных, чтобы хранить доверенные проекты.
workbench-trust-write = Не удалось сохранить доверенные проекты: { $error }
workbench-trust-not-folder = { $path } — не папка.
workbench-trust-run = { $name } не запускается: проект открыт в безопасном режиме, в нём ничего не запускается и не отлаживается. Сначала доверьте проекту («Доверять проекту»).
workbench-trust-before-launch = { $name } перед запуском выполняет программы, а проект не доверенный: сначала доверьте проекту («Доверять проекту»).
workbench-trust-agent-build-setting = Агент не может менять { $key }: этот параметр выбирает программу или аргументы JVM, с которыми запускается сборка. Человек может изменить его в настройках.
workbench-trust-build-load = Проект { $project } не отмечен как доверенный, поэтому сборка не загружена: при загрузке выполняется код самой сборки. Чтобы загрузить её, отметьте проект как доверенный.
workbench-trust-artifacts = Артефакты проекта { $project } собираются его сборкой и записываются туда, куда указывает проект, а проект не доверенный: сначала доверьте проекту («Доверять проекту»).
workbench-trust-file-watchers = Наблюдатели за файлами проекта { $project } пришли вместе с проектом, а проект не доверенный: доверьте проекту («Доверять проекту»), чтобы запускать их.
workbench-profile-unsupported = Конфигурации вида «{ $kind }» нельзя запустить с профилировщиком.
workbench-profile-agent-outside = Агент может открыть профиль только внутри проекта, а { $path } вне его.
workbench-profile-no-run = Запуск с профилировщиком не начался.
workbench-profile-none = Профиль не показан; сначала запустите конфигурацию с профилировщиком или откройте файл pprof.
workbench-profile-console = Профиль: выборок { $samples }, самая затратная функция { $function }
workbench-profile-console-empty = Профиль: выборок нет; возможно, запуск был слишком коротким.
workbench-profile-console-error = Не удалось прочитать профиль: { $error }
workbench-profile-unknown-metric = В профиле нет метрики «{ $metric }».
workbench-profile-unknown-frame = В профиле нет кадра { $frame }.
workbench-profile-no-source = Исходного кода { $function } нет на этом компьютере.
workbench-coverage-unsupported = Конфигурации вида «{ $kind }» нельзя запустить с покрытием.
workbench-coverage-no-run = Запуск с покрытием не начался.
workbench-coverage-none = Покрытие не показано; сначала запустите конфигурацию с покрытием.
workbench-coverage-console = Покрытие: { $percent }% строк в файлах: { $files }
workbench-coverage-console-error = Не удалось прочитать данные о покрытии: { $error }
workbench-coverage-export-failed = Не удалось записать отчёт о покрытии в { $path }: { $error }
workbench-coverage-html-title = Покрытие: { $name }
workbench-coverage-html-summary = Покрыто { $percent }% строк ({ $hit } из { $total })
workbench-coverage-html-file = Файл
workbench-coverage-html-percent = Строки, %
workbench-coverage-html-lines = Покрытые строки
workbench-coverage-html-hits = Выполнений: { $hits }
workbench-error-no-tab-to-detach = Нет вкладки, которую можно открыть в новом окне.
workbench-trust-agent-how = Агент не может доверять проекту: человек открывает его один раз в окне IDE с этой же папкой данных и доверяет там или добавляет доверенное расположение; IDE без окна затем следует этому ответу.
workbench-rearrange-unsupported = У «Переупорядочить код» нет правил упорядочивания для языка этого файла.
workbench-code-style-unknown = У { $language } нет настроек стиля кода.
workbench-code-style-no-file = { $language } не хранит стиль кода в файле проекта.
workbench-code-style-invalid = { $option } — не настройка файла форматировщика, или её значение недопустимо.
workbench-trust-console = Консоль с классами проекта { $project } собирает проект и загружает его код, а проект не доверенный: сначала доверьте проекту («Доверять проекту»).
workbench-ide-settings-person-only = Импортировать настройки и восстанавливать значения по умолчанию может только человек: обе операции заменяют настройки целиком, включая то, что агенту менять нельзя, например файлы, скрытые от агентов. Попросите человека воспользоваться меню «Файл → Управление настройками IDE».
workbench-trust-agent-data-folder = Агент не может менять { $path }: это папка данных IDE, где хранятся настройки и список доверенных проектов. Человек может изменить их в самой IDE.

workbench-inspect-scope-project = Весь проект
workbench-inspect-scope-directory = Папка { $directory }
workbench-inspect-scope-uncommitted = Незакоммиченные файлы
workbench-inspect-scope-files = { $count ->
    [one] { $count } файл
    [few] { $count } файла
   *[other] { $count } файлов
}
workbench-inspect-task = Проверка: { $scope }
workbench-inspect-progress = Готово проверок: { $done } из { $total }
workbench-inspect-done = { $count ->
    [one] Найдена { $count } проблема
    [few] Найдено { $count } проблемы
   *[other] Найдено { $count } проблем
}
workbench-inspect-cancelled = Отменено
workbench-inspect-power-save = Включён режим энергосбережения: языковые серверы не опрашивались
workbench-inspect-no-answer = Языковой сервер не сообщил об этом файле вовремя
workbench-inspect-unknown-run = Нет результатов проверки с номером { $run }
workbench-inspect-unknown-item = В этих результатах нет элемента { $item }
workbench-inspect-no-results = Проверка ещё не запускалась
workbench-inspect-other = Другие проблемы
workbench-inspect-results-title = Результаты проверки: { $scope }
workbench-inspect-export-failed = Не удалось записать { $path }: { $error }
workbench-inspect-fix-task = { $count ->
    [one] Применяется { $count } быстрое исправление
    [few] Применяются { $count } быстрых исправления
   *[other] Применяется { $count } быстрых исправлений
}
workbench-inspect-fix-done = Исправлено: { $fixed }, пропущено: { $skipped }
workbench-inspect-fix-none = Языковой сервер не предлагает здесь быстрых исправлений
workbench-inspect-fix-gone = Об этой проблеме сервер больше не сообщает
workbench-suppress-statement = Подавить { $inspection } для инструкции
workbench-suppress-function = Подавить { $inspection } для функции
workbench-suppress-file = Подавить { $inspection } для файла
workbench-suppress-unavailable = { $inspection } нельзя подавить здесь

workbench-cleanup-task = Очистка кода: { $scope }
workbench-cleanup-found = { $count ->
    [one] Проблем для очистки: { $count }
   *[other] Проблем для очистки: { $count }
}
workbench-cleanup-done = Очистка кода: исправлено { $fixed }, пропущено { $skipped }, изменено файлов: { $files }
workbench-profile-write-failed = Не удалось записать профиль инспекций: { $error }
workbench-profile-no-ide-folder = У этой IDE нет папки для профилей инспекций
workbench-profile-unknown = Профиля инспекций с именем { $name } нет
workbench-profile-unreadable = { $file } не является профилем инспекций
workbench-profile-bad-name = У профиля должно быть имя
workbench-profile-exists = Профиль с именем { $name } уже есть
workbench-profile-builtin = Project Default нельзя переименовать или удалить
workbench-cleanup-undo = Отменить
workbench-cleanup-nothing-to-undo = Нет очистки кода, которую можно отменить
workbench-fix-safe-mode = Это исправление запускает программу проекта, а проекту нет доверия: сначала доверьте ему.
workbench-run-agent-missing = В classpath, который собрал этот запуск, нет jar { $prefix }…: программа запускается без этого агента.
workbench-convert-unsupported = Ни один плагин не преобразует код .{ $from } в .{ $to }
workbench-convert-failed = Плагин не смог преобразовать этот код
workbench-convert-exists = { $path } уже есть: сначала переименуйте или удалите его
workbench-convert-read-failed = Не удалось прочитать { $path }: { $error }
workbench-convert-write-failed = Не удалось записать { $path }: { $error }
workbench-schema-download-off = Скачивание схем выключено: { $uri } не скачивается. Человек может разрешить скачивание схем, названных в файле, в Settings → Languages & Frameworks → Schemas.
workbench-plugin-run-kind = Цель плагина
workbench-plugin-run-field-plugin = Плагин
workbench-plugin-run-field-file = Файл
workbench-plugin-run-field-program = Программа
workbench-plugin-run-undeclared = Плагин { $plugin } не объявлял программу { $program }, поэтому она не запускается.
workbench-plugin-run-not-found = { $program } не установлена: её нет в PATH и в обычных папках.
workbench-plugin-run-agent-none = Агентам нельзя запускать цели плагина { $plugin } ({ $name }): человек может разрешить это в Settings → Plugins.
workbench-plugin-run-person-only = { $name } выполняет `{ $line }`, а это меняет состояние или инфраструктуру: запускает только человек, из окна IDE, прочитав команду.
workbench-diff-live = Сейчас ({ $place }): { $object }
workbench-diff-live-plain = Сейчас: { $object }
workbench-diff-file = После применения { $file }: { $object }
workbench-diff-same = Различий нет: то, что описано в файле, уже есть.
workbench-diff-opened = { $count ->
    [one] Отличается { $count } объект: он открыт в окне сравнения.
    [few] Отличаются { $count } объекта: они открыты в окне сравнения.
   *[other] Отличаются { $count } объектов: они открыты в окне сравнения.
}
workbench-diff-failed = Сравнение не удалось (код выхода { $code }).
workbench-document-opened = Вывод открыт только для чтения как { $file }.
workbench-document-cut = Вывод длиннее { $size } МиБ: документ показывает его первые { $size } МиБ.
workbench-document-title = Вывод { $command }
workbench-index-state-from = Состояние из { $context }
workbench-index-state-needs = Для состояния кластера нужен плагин { $plugin }, а он не установлен или выключен.
workbench-index-state-untrusted = Состояния кластера нет: проекту не доверяют, а чтение состояния запускает программу.
workbench-plugin-run-person-asked = { $name } выполняет `{ $line }`, а это меняет состояние или инфраструктуру: окно IDE сначала спрашивает человека.
workbench-plugin-run-agent-ask = ИИ-агент хочет запустить { $name } (плагин { $plugin }).
workbench-plugin-run-agent-asked = { $name } ждёт согласия человека в уведомлении; когда оно будет дано, запустите снова с этим подтверждением.
workbench-plugin-run-allow = Разрешить один раз
workbench-plugin-run-access-person = Что могут запускать агенты, решает только человек.
workbench-plugin-run-no-question = Агент не просил этот запуск, или ответ уже дан.
workbench-tool-unknown-plugin = Среди установленных плагинов нет плагина с id { $plugin }.
workbench-index-unknown = Ни один плагин с id { $plugin } не объявляет индекс проекта.
workbench-index-no-target = В этом репозитории нет { $target }.
workbench-index-usages = Использования { $kind } { $name }
workbench-index-usages-scope = Файлы проекта
workbench-tool-unknown-window = У плагина { $plugin } нет окна инструментов { $window }.
workbench-tool-unknown-section = В окне инструментов нет раздела { $section }.
workbench-tool-unknown-action = В разделе нет действия { $action }.
workbench-tool-unknown-row = Строка { $row } в этом разделе не была показана; получите строки заново и передайте id одной из них.
workbench-tool-needs-rows = Выберите строки, с которыми работает это действие.
workbench-tool-bad-argument = Аргумент плагина { $arg } не называет ни одного столбца раздела.
workbench-tool-untrusted = Проекту не доверяют, поэтому окна инструментов плагинов ничего в нём не запускают.
workbench-tool-confirm = { $action }: это нельзя отменить. Сначала подтвердите.
workbench-tool-agent-destroy = AI-агент хочет выполнить: { $action }. Это нельзя отменить.
workbench-tool-busy = Действие в этом разделе ещё выполняется; дождитесь его окончания.
workbench-tool-failed = { $action } не удалось: { $error }
workbench-tool-choice-gone = Значения { $value } больше нет среди вариантов «{ $choice }»: выберите другое в заголовке окна.
workbench-tool-choice-none = Для «{ $choice }» нечего выбрать.
workbench-tool-choose = Сначала выберите «{ $choice }» в заголовке окна.
workbench-tool-list-again = Эти строки получены при другом выборе в заголовке окна; получите их снова.
workbench-tool-more-rows = ещё { $count }
workbench-tool-agent-confirm = AI-агент спрашивает: { $question }
workbench-tool-choose-person = Выбирать в заголовке окна инструментов можете только вы; AI-агенты читают выбор из plugins.toolWindow.rows.
workbench-tool-unknown-choice = В окне инструментов нет выбора { $choice }.
workbench-tool-unknown-option = { $value } нет среди вариантов «{ $choice }»; получите строки снова.
workbench-schema-download-untrusted = { $uri } не скачивается: проекту не доверяют.
workbench-schema-download-unannounced = { $uri } не скачивается: ни одно окно не смогло сначала сказать об этом.
workbench-schema-download-failed = { $uri } не удалось скачать: { $error }
workbench-schema-downloading = Скачивается схема { $uri }, названная в файле.
workbench-schema-mapping-incomplete = Сопоставлению схемы нужны схема и хотя бы один шаблон файлов ({ $schema }).
workbench-schema-downloads-person = Разрешать скачивание схем может только человек.
workbench-tool-input-unknown = Действие не принимает значение { $input }.
workbench-tool-input-range = { $input }: нужно целое число от { $min } до { $max }.
workbench-tool-input-forward = { $input }: нужны локальный порт от 1024 до 65535 и удалённый порт от 1 до 65535.
workbench-tool-input-agent = Агенты не передают значения действиям окон инструментов плагинов: их вводит человек в диалоге окна.
workbench-tool-agent-values = AI-агент хочет выполнить { $action }; введите значения.
workbench-tool-agent-values-in = AI-агент хочет выполнить { $action } в { $scope }; введите значения.
workbench-tool-agent-values-asked = { $name } ждёт значений, которые человек введёт в диалоге окна; вызовите снова с этим подтверждением, когда они будут введены.
workbench-tool-enter-values = Ввести значения…
