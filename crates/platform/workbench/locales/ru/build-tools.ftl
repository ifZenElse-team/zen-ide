# Maven и Gradle (ADR-0149).

workbench-build-cancelled = Отменено.
workbench-build-failed = { $program } завершился с кодом { $code }:
    { $output }

workbench-maven-kind = Maven
workbench-maven-task = Выполнить цель Maven
workbench-maven-field-pom = Файл POM:
workbench-maven-field-goals = Запуск (цели):
workbench-maven-field-profiles = Профили:
workbench-maven-field-offline = Работать офлайн
workbench-maven-field-skip-tests = Пропустить тесты
workbench-maven-no-goals = укажите цели или фазы, например clean install
workbench-maven-anything-help = Цели и фазы Maven: mvn clean install, mvn test
workbench-maven-not-found = Maven не найден: в проекте нет Maven Wrapper (mvnw), а на этом компьютере нет ни mvn, ни загрузки обёртки. Установите Maven или нажмите «Скачать Maven».
workbench-maven-no-pom = В корне проекта нет pom.xml.
workbench-maven-no-module = { $pom } — не pom.xml этого проекта.
workbench-maven-tree-task = Загрузка дерева зависимостей Maven
workbench-maven-no-coordinates = Поставьте курсор на зависимость, плагин или родителя в pom.xml или укажите координаты.
workbench-maven-no-version = Версия { $coordinates } не записана в этом pom.xml (ею управляет родитель или BOM); откройте зависимость из дерева зависимостей.
workbench-maven-not-downloaded = { $coordinates } ещё нет в локальном репозитории Maven; «Перезагрузить все проекты Maven» скачает её.
workbench-maven-download-task = Загрузка Maven { $version }
workbench-maven-download-no-folder = У IDE нет папки данных, куда скачать Maven.
workbench-maven-download-checksum = Загруженный { $url } не совпадает со своим SHA-512; ничего не установлено.
workbench-maven-download-unavailable = Здесь Maven скачать нельзя.
workbench-maven-download-done = Maven установлен в { $folder }.

workbench-gradle-kind = Gradle
workbench-gradle-task = Выполнить задачу Gradle
workbench-gradle-field-project = Проект Gradle:
workbench-gradle-field-tasks = Запуск (задачи и аргументы):
workbench-gradle-no-tasks = укажите задачи, например build или :app:run
workbench-gradle-not-found = Gradle не найден: в сборке нет Gradle Wrapper (gradlew), а на этом компьютере нет ни gradle, ни загрузки обёртки. Добавьте wrapper в сборку, установите Gradle или нажмите «Скачать Gradle»; какой Gradle запускать, выбирается в Настройки → Build Tools → Gradle.
workbench-gradle-anything-help = Задачи Gradle: gradle build, gradle test, gradle :app:run
workbench-gradle-no-build = В корне проекта нет settings.gradle(.kts) или build.gradle(.kts).
workbench-gradle-tasks-task = Загрузка задач Gradle

workbench-sync-maven-read = Чтение файлов pom.xml
workbench-sync-gradle-read = Чтение скриптов Gradle
workbench-sync-import = Импорт в языковой сервер Java
workbench-sync-no-java-server = Языкового сервера Java нет: плагин Java выключен или не установлен.
workbench-sync-java-server-off = { $server } выключен, поэтому проект в него не импортирован.
workbench-sync-java-server-stopped = { $server } остановлен, поэтому проект в него не импортирован.
workbench-sync-java-server-missing = { $server } не установлен, поэтому проект в него не импортирован.
workbench-sync-import-long = Импорт идёт долго; он продолжается в фоне.
workbench-sync-fix-install-server = Установить { $server }
workbench-sync-fix-restart-server = Перезапустить
workbench-sync-maven-dependencies = Разрешение зависимостей
workbench-sync-gradle-tasks = Загрузка задач сборки
workbench-sync-fix-download-maven = Скачать Maven
workbench-sync-fix-download-gradle = Скачать Gradle
workbench-sync-check = Проверить проект
workbench-block-not-trusted = Этому проекту нет доверия: его сценарии сборки не запускаются сами, а Java работает без Lombok и обработчиков аннотаций.
workbench-block-no-jdk = На этом компьютере нет JDK.
workbench-block-jdk = Сборка просит Java { $wanted }, а работает на JDK { $major }.
workbench-block-use-jdk = Использовать JDK { $level }
workbench-sync-fix-jdk = Выбрать JDK
workbench-sync-fix-offline = Работать офлайн
workbench-maven-not-linked = { $pom } — не связанный проект Maven; модуль уходит вместе со своим проектом.
workbench-maven-no-pom-file = Нет pom.xml по пути { $pom }.
workbench-maven-effective-task = Вычисление эффективного POM
workbench-build-triggers-write = Не удалось сохранить задачи сборки: { $error }
workbench-build-triggers-not-lines = Задачу типа { $kind } нельзя выполнять вокруг сборки.

workbench-dependency-task = Анализ зависимостей
workbench-dependency-no-build = В проекте нет сборки Maven или Gradle для анализа.
workbench-dependency-no-module = В сборке нет модуля { $module }.
workbench-dependency-not-loaded = Зависимости ещё не проанализированы: сначала запустите Analyze Dependencies.
workbench-dependency-no-declaration = Ни один файл сборки проекта не объявляет { $dependency }.
workbench-dependency-exclude-direct = { $dependency } здесь не приходит с другой зависимостью: удалите её из файла сборки.
workbench-dependency-bad-name = { $dependency } не похоже на имя зависимости.
workbench-dependency-edit-failed = Не удалось изменить файл сборки: { $error }
workbench-gradle-model-empty = Gradle завершился, но не описал сборку.
workbench-gradle-no-build-at = В { $folder } нет сборки Gradle.
workbench-gradle-not-linked = { $folder } не подключена как сборка Gradle.
workbench-build-triggers-sync-gradle = До и после синхронизации выполняются только задачи Gradle.
