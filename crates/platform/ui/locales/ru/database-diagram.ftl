# Zen UI: ER-диаграммы в рабочей области «База данных».

ide-database-view-diagram = Показать диаграмму
ide-database-diagram-canvas = ER-диаграмма
ide-database-diagram-zoom-in = Увеличить
ide-database-diagram-zoom-out = Уменьшить
ide-database-diagram-actual-size = Реальный размер (100%)
ide-database-diagram-fit = Вписать в окно
ide-database-diagram-export = Экспорт
ide-database-diagram-export-png = Изображение PNG…
ide-database-diagram-export-svg = Изображение SVG…
ide-database-diagram-export-mermaid = Текст Mermaid…
ide-database-diagram-copy-mermaid = Копировать как Mermaid
ide-database-diagram-copied = Текст Mermaid скопирован
ide-database-diagram-exporting = Экспорт…
ide-database-diagram-exported = Экспортировано в { $path }
ide-database-diagram-export-failed = Экспорт не удался: { $error }
ide-database-diagram-cancel = Отмена
ide-database-diagram-computing = Диаграмма раскладывается…
ide-database-diagram-empty = Нет таблиц: структура ещё не прочитана, или в схеме нет таблиц.
ide-database-diagram-counts = { $entities ->
    [one] { $entities } таблица
    [few] { $entities } таблицы
   *[many] { $entities } таблиц
} · { $relations ->
    [one] { $relations } связь
    [few] { $relations } связи
   *[many] { $relations } связей
}
ide-database-diagram-partial = часть столбцов ещё не прочитана
ide-database-diagram-more = ещё { $count }
ide-database-diagram-open-hint = двойной щелчок открывает
