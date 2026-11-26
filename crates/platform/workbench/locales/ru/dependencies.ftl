workbench-deps-group = Зависимости
workbench-deps-own-repositories = собственные репозитории проекта
workbench-deps-search = Искать библиотеки в репозиториях
workbench-deps-search-description = Окно Dependencies и дополнение в файлах сборки ищут библиотеки в поисковом индексе Maven Central, а их версии читают из maven-metadata.xml.
workbench-deps-check-updates = Проверять новые версии зависимостей
workbench-deps-check-updates-description = Читает maven-metadata.xml каждой объявленной зависимости из репозиториев модуля, чтобы показать новые версии.
workbench-deps-check-vulnerabilities = Проверять зависимости на известные уязвимости
workbench-deps-check-vulnerabilities-description = Спрашивает OSV о каждой зависимости, которая есть в публичном репозитории, чтобы отметить уязвимые версии и предложить ближайшую безопасную.
workbench-deps-no-module = { $module } не является модулем этого проекта; их перечисляет dependencies.project.list.
workbench-deps-reload-untrusted = Проект не отмечен как доверенный, поэтому он не загружен заново; после разрешения используйте Load Changes.
workbench-deps-already = { $dependency } уже объявлена с такой областью.
workbench-deps-not-declared = { $dependency } не объявлена в этом модуле.
workbench-deps-managed-outside = Версией { $dependency } управляют вне этого проекта, поэтому здесь её изменить нельзя.
workbench-deps-exclude-unsupported = Исключение зависимости пока недоступно.
workbench-deps-vulnerable = { $count ->
    [one] У этой версии { $count } известная уязвимость
    [few] У этой версии { $count } известные уязвимости
   *[other] У этой версии { $count } известных уязвимостей
}
workbench-deps-outdated = Доступна новая версия: { $version }
workbench-deps-fix-upgrade = Обновить до { $version }
workbench-deps-fix-safe = Обновить до незатронутой версии { $version }
workbench-deps-latest = последняя
workbench-deps-changed-outside = Файл { $file } изменился на диске, пока готовилась эта правка, поэтому в него ничего не записано. Попробуйте ещё раз.
