ide-plugin-tool-refresh = Обновить
ide-index-tree = Дерево
ide-index-graph = Граф
ide-index-filter = Фильтр по имени, виду или пути
ide-index-indexing = Чтение репозитория…
ide-index-empty = В этом проекте нет объектов GitOps: файлы Flux, Argo CD, Kustomize и Helm не найдены.
ide-index-truncated = Проект превысил пределы индекса; часть его не показана.
ide-index-state-none = Состояния кластера пока нет: оно читается с контекстом окна Kubernetes.
ide-index-state-missing = В кластере нет { $kind } { $name }.
ide-plugin-tool-empty = Здесь ничего нет.
ide-plugin-tool-busy = Выполняется…
ide-plugin-tool-confirm-rows = { $count ->
    [one] Выбранный элемент будет удалён. Это нельзя отменить.
    [few] { $count } выбранных элемента будут удалены. Это нельзя отменить.
   *[other] { $count } выбранных элементов будут удалены. Это нельзя отменить.
}
ide-plugin-tool-confirm-section = Это нельзя отменить.
ide-plugin-tool-choice = { $choice }:
ide-plugin-tool-choice-none = Не выбрано
ide-plugin-tool-choice-source = { $choice } из: { $files }
ide-plugin-tool-choice-missing = { $path } (нет файла)
ide-plugin-tool-and = { $first } и { $last }
ide-plugin-tool-moved = { $windows } теперь { $count ->
    [one] открывается
   *[other] открываются
} { $side ->
    [left] слева
    [right] справа
   *[bottom] внизу
}; команда Restore Default Layout { $count ->
    [one] переставит его
   *[other] переставит их
} туда.
ide-plugin-tool-input-range = Целое число от { $min } до { $max }.
ide-plugin-tool-input-forward = Локальный порт от 1024 до 65535 и удалённый порт от 1 до 65535.
ide-plugin-tool-input-forward-hint = локальный : удалённый, только на 127.0.0.1
