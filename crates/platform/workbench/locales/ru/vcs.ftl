# Системы контроля версий (zen_workbench::vcs).

vcs-error-no-repository = Проект не находится в Git-репозитории.
vcs-error-outside-repository = { $path } не находится в репозитории.
vcs-error-no-git = Не удаётся запустить Git: { $reason }. Установите Git и проверьте, что он есть в PATH.
vcs-error-unsupported = Контроль версий здесь недоступен.
vcs-error-detached = HEAD отсоединён. Сначала переключитесь на ветку.
vcs-error-no-remote = У репозитория нет удалённого репозитория.
vcs-error-busy = Выполняется другая операция Git. Дождитесь её завершения.
vcs-error-branch-name = Введите имя ветки.
vcs-error-revision = Введите тег или ревизию.
vcs-error-no-log-tab = Такой вкладки журнала нет.
vcs-error-tag-name = Введите имя тега.
vcs-error-already-on-branch = Этот коммит уже есть в текущей ветке.
vcs-error-cherry-pick-conflicts = Cherry-pick остановлен: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Репозиторий оставлен в том состоянии, в каком его оставил Git. Разрешите их («Разрешить…» открывает редактор конфликтов), затем нажмите «Продолжить» или «Прервать», чтобы вернуть всё как было.
vcs-error-revert-conflicts = Отмена коммита остановлена: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Репозиторий оставлен в том состоянии, в каком его оставил Git. Разрешите их («Разрешить…» открывает редактор конфликтов), затем нажмите «Продолжить» или «Прервать», чтобы вернуть всё как было.
vcs-error-merge-conflicts = Слияние остановлено: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Репозиторий оставлен в том состоянии, в каком его оставил Git. Разрешите их («Разрешить…» открывает редактор конфликтов), затем нажмите «Продолжить» или «Прервать», чтобы вернуть всё как было.
vcs-error-rebase-conflicts = Перебазирование остановлено: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Репозиторий оставлен в том состоянии, в каком его оставил Git. Разрешите их («Разрешить…» открывает редактор конфликтов), затем нажмите «Продолжить» или «Прервать», чтобы вернуть всё как было.
vcs-error-not-fast-forward = Ветку { $branch } нельзя перемотать вперёд до { $target }: в ней есть свои коммиты. Выполните слияние или перебазирование.
vcs-error-no-branch = Локальной ветки { $branch } нет.
vcs-error-no-ref = Ветки или тега { $name } нет.
vcs-error-focus-name = Укажите ветку или удалённый репозиторий, которые нужно скрыть или показать одни.
vcs-error-no-refs = На этом коммите нет веток и тегов.
vcs-error-ref-action = Это действие недоступно для этой ветки или тега.
vcs-error-no-tracking = Ни одна локальная ветка не отслеживает { $branch }.
vcs-error-checkout-changes = Сначала закоммитьте или спрячьте изменения: для этого нужно переключиться на { $branch }, и изменения перешли бы туда же.
vcs-error-drop-same = Перетащите ветку на другую ветку.
vcs-error-drop-nothing = Здесь перетаскивание ничего не сделает: слить можно в локальную ветку, перебазировать — локальную ветку.
vcs-error-drop-action = Этот пункт не подходит для этих веток.
vcs-error-stash-apply-conflicts = Применение отложенных изменений остановлено: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Отложенные изменения сохранены. Разрешите конфликты («Разрешить…» открывает редактор конфликтов), затем нажмите «Продолжить» или «Прервать», чтобы вернуть файлы как были.
vcs-error-stash-pop-conflicts = Извлечение отложенных изменений остановлено: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Git сохраняет stash, если он применился с конфликтами. Разрешите конфликты («Разрешить…» открывает редактор конфликтов), затем нажмите «Продолжить» — stash будет удалён — или «Прервать», чтобы вернуть файлы как были.
vcs-error-nothing-to-stash = Нет локальных изменений, которые можно отложить.
vcs-error-nothing-staged = Сначала подготовьте изменения для коммита.
vcs-error-no-stash = Отложенных изменений { $name } нет.
vcs-error-no-stashes = Отложенных изменений нет.
vcs-error-stash-message = Введите описание отложенных изменений.
vcs-error-compare-two = Чтобы сравнить, выберите ⌘-щелчком два коммита или коммит и незафиксированные изменения.
vcs-error-not-in-working-tree = Файла { $path } нет в рабочем дереве.
vcs-conflicts-more = { $count ->
    [one] {" "}и ещё { $count }
   *[other] {" "}и ещё { $count }
}

vcs-progress-update = Обновление…
vcs-progress-fetch = Получение изменений…
vcs-progress-push = Отправка…
vcs-progress-checkout = Переключение…
vcs-progress-merge = Слияние…
vcs-progress-rebase = Перебазирование…
vcs-progress-cherry-pick = Перенос коммита…
vcs-progress-revert = Отмена коммита…
vcs-progress-reset = Сброс…
vcs-progress-commit = Создание коммита…
vcs-progress-delete-remote = Удаление удалённой ветки…

vcs-update-up-to-date = Все файлы актуальны
vcs-update-done = { $count ->
    [one] Проект обновлён: { $count } коммит
    [few] Проект обновлён: { $count } коммита
   *[other] Проект обновлён: { $count } коммитов
}
vcs-fetch-done = Изменения получены
vcs-branch-updated = Ветка { $branch } обновлена
vcs-push-done = { $count ->
    [one] Отправлен { $count } коммит в { $branch }
    [few] Отправлено { $count } коммита в { $branch }
   *[other] Отправлено { $count } коммитов в { $branch }
}
vcs-push-new-branch = { $branch } отправлена как новая ветка
vcs-push-nothing = Всё актуально
vcs-push-remote-moved = Ветка { $branch } изменилась, пока был открыт этот диалог. Посмотрите её новые коммиты и повторите Push.
vcs-push-tag-done = Тег { $tag } отправлен в { $remote }
vcs-remote-branch-deleted = Ветка { $branch } удалена в удалённом репозитории

vcs-error-outside = { $path } не находится в репозитории.
vcs-error-empty-message = Введите сообщение коммита.
vcs-error-sign-off-identity = Для подписи коммита нужны имя и e-mail в Git (user.name и user.email).
vcs-error-nothing-to-commit = Выберите изменения для коммита.
vcs-error-no-file = Откройте файл или укажите, какой.
vcs-error-not-open = Файл не открыт в редакторе.
vcs-error-no-change = Здесь нет изменений.
vcs-commit-done = { $count ->
    [0] Коммит исправлен: { $subject }
    [one] { $count } файл закоммичен: { $subject }
    [few] { $count } файла закоммичено: { $subject }
   *[other] { $count } файлов закоммичено: { $subject }
}
vcs-diff-head = HEAD
vcs-diff-yours = Ваша версия
vcs-diff-parent = Родитель { $commit }
vcs-diff-staged = Подготовленная версия
vcs-error-nothing-staged = Сначала подготовьте изменения для коммита.
vcs-error-one-file = Укажите один файл, строки которого нужно взять.
vcs-error-not-text = { $path } — не текстовый файл: подготовьте или отмените его целиком.
vcs-error-no-lines = В этих строках нет изменений.

# Отмена и повтор операций Git (vcs.history.*, ADR-0064).
vcs-history-op-checkout = Переключение на «{ $subject }»
vcs-history-op-commit = Коммит «{ $subject }»
vcs-history-op-amend = Исправление коммита «{ $subject }»
vcs-history-op-reset = Сброс «{ $target }» до { $subject }
vcs-history-op-branch-create = Создание ветки «{ $subject }»
vcs-history-op-branch-delete = Удаление ветки «{ $subject }»
vcs-history-op-merge = Слияние «{ $subject }» в «{ $target }»
vcs-history-op-rebase = Перебазирование «{ $target }» на «{ $subject }»
vcs-history-op-cherry-pick = Cherry-pick «{ $subject }»
vcs-history-op-revert = Отмена коммита «{ $subject }»
vcs-history-op-fast-forward = Перемотка «{ $subject }» вперёд
vcs-history-op-interactive-rebase = Интерактивное перебазирование «{ $target }» на «{ $subject }»
vcs-history-op-cherry-pick-many = { $count ->
    [1] Cherry-pick «{ $subject }»
   *[other] Cherry-pick коммитов: { $count }
}
vcs-history-op-discard = { $count ->
    [1] Отмена изменений в «{ $subject }»
   *[other] Отмена изменений в { $count } файлах
}
vcs-history-op-rollback = { $count ->
    [1] Откат «{ $subject }»
   *[other] Откат { $count } файлов
}
vcs-history-nothing-undo = Нет операции Git, которую можно отменить.
vcs-history-nothing-redo = Нет операции Git, которую можно повторить.
vcs-history-not-next-undo = Сейчас отменяется операция «{ $operation }», а не №{ $id }.
vcs-history-not-next-redo = Сейчас повторяется операция «{ $operation }», а не №{ $id }.
vcs-history-refused-undo = Нельзя отменить «{ $operation }»: { $reason }
vcs-history-refused-redo = Нельзя повторить «{ $operation }»: { $reason }
vcs-history-reason-in-progress = выполняется { $operation }. Сначала завершите или прервите эту операцию.
vcs-history-reason-head = HEAD теперь на { $now }, а не на { $expected }. Репозиторий с тех пор изменился.
vcs-history-reason-branch = { $branch } теперь на { $now }, а не на { $expected }. Репозиторий с тех пор изменился.
vcs-history-reason-deleted = ветка { $branch } с тех пор удалена.
vcs-history-reason-created = теперь есть ветка { $branch }.
vcs-history-reason-file = файл { $file } с тех пор изменился, и его изменения были бы потеряны.
vcs-history-failed-undo = Не удалось отменить «{ $operation }»: { $message }
vcs-history-failed-redo = Не удалось повторить «{ $operation }»: { $message }
vcs-changelist-default = Изменения
vcs-error-changelist-name = У списка изменений должно быть имя.
vcs-error-changelist-exists = Список изменений «{ $name }» уже есть.
vcs-error-no-changelist = Нет списка изменений «{ $name }».
vcs-error-changelist-active = «{ $name }» — активный список изменений: сначала сделайте активным другой.
vcs-error-no-files = Укажите файлы, которые нужно переместить.
vcs-error-not-in-changelist = { $path } — не изменённый файл из списка изменений.
vcs-error-nothing-to-shelve = Нет изменений, которые можно убрать на полку.
vcs-error-shelve-rollback = Часть изменений осталась в рабочем дереве: { $reason }. Всё сохранено в отложенных изменениях «{ $name }», а Undo вернёт файлы.
vcs-error-not-changed = В { $path } нет изменений.
vcs-error-shelve-conflicted = В { $path } есть конфликты: разрешите их, прежде чем убирать файл на полку.
vcs-error-no-shelf = На полке нет изменений с номером { $id }.
vcs-error-not-shelved = { $path } нет среди этих изменений на полке.
vcs-error-shelf-name = У изменений на полке должно быть имя.
vcs-error-invalid-branch = «{ $name }» не подходит как имя ветки.
vcs-error-invalid-tag = «{ $name }» не подходит как имя тега.
vcs-error-invalid-remote = «{ $name }» не подходит как имя удалённого репозитория.
vcs-error-invalid-revision = «{ $name }» не подходит как ревизия.
vcs-error-unshelve-conflicts = Возврат с полки остановлен: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Изменения остаются на полке. Разрешите конфликты в файлах.
vcs-shelf-diff-base = Базовая версия
vcs-shelf-diff-shelved = Версия на полке
vcs-history-op-shelve = Убрать на полку «{ $subject }»
vcs-history-op-unshelve = Вернуть с полки «{ $subject }»
vcs-history-op-apply-patch = Применить патч «{ $subject }»

# Создание и применение патчей (vcs.patch.*, ADR-0117).
vcs-patch-name = Патч
vcs-patch-conflict-current = Текущая версия
vcs-patch-conflict-patch = Патч
vcs-patch-diff-current = Текущая версия
vcs-patch-diff-patched = После патча
vcs-patch-error-revision = Укажите коммит, из которого нужно сделать патч.
vcs-patch-error-shelf = Укажите изменения на полке, из которых нужно сделать патч.
vcs-patch-error-no-changes = Нет изменений, которые можно положить в патч.
vcs-patch-error-nothing = Ни одного из этих файлов нет в патче.
vcs-patch-error-encoding = { $name } — не кодировка.
vcs-patch-error-base = Базовый путь { $path } не внутри репозитория и не выше него.
vcs-patch-error-no-input = Укажите файл патча, текст патча или буфер обмена.
vcs-patch-error-empty = В патче нет изменений.
vcs-patch-error-hunk = Строка { $line } патча — не заголовок фрагмента.
vcs-patch-error-truncated = Фрагмент в строке { $line } патча оборван.
vcs-patch-error-unsafe = Патч называет «{ $path }» — это вне проекта или в .git; патч не применён.
vcs-patch-error-strip = В «{ $path }» меньше папок, чем нужно отбросить.
vcs-patch-error-outside = { $path } вне базового пути.
vcs-patch-error-large = Патч слишком большой.
vcs-patch-error-not-in-patch = { $path } нет в патче.
vcs-patch-error-not-applied = Патч не удалось применить: { $files }.
vcs-patch-reason-symlink = Путь проходит через символическую ссылку; патч туда не пишет.
vcs-patch-reason-special = Патч создаёт или меняет символическую ссылку или подмодуль, а IDE такое не применяет.
vcs-patch-reason-no-data = Патч говорит, что двоичный файл изменился, но данных для него не содержит.
vcs-patch-reason-not-text = Файл не в UTF-8, поэтому то, что не применилось, слить нельзя.
vcs-patch-reason-delete-differs = У удаляемого файла другое содержимое, чем то, что удаляет патч.
vcs-patch-reason-target-exists = Там, куда патч переносит или копирует файл, уже есть файл.
vcs-error-patch-conflicts = Не все фрагменты применились: { $count ->
    [one] конфликт в { $count } файле
   *[other] конфликты в { $count } файлах
}: { $files }. Объедините их в редакторе конфликтов.

# Редактор интерактивного перебазирования (vcs.rebase.*, ADR-0065).
vcs-rebase-problem-meld = { $commit } нельзя объединить с предыдущим: перед ним нет оставленного коммита.
vcs-rebase-problem-message = Введите сообщение для { $commit }.
vcs-rebase-problem-nothing = Все коммиты исключены: переносить нечего.
vcs-rebase-error-no-step = В интерактивном перебазировании нет такого коммита.
vcs-rebase-error-closed = Редактор интерактивного перебазирования не открыт.
vcs-rebase-error-in-progress = Выполняется { $operation }. Сначала завершите или прервите эту операцию.
vcs-rebase-error-nothing = В репозитории ещё нет коммитов.
vcs-rebase-error-on-branch = { $commit } уже есть в текущей ветке.
vcs-rebase-error-merge = { $commit } — коммит слияния: такие коммиты здесь не переносятся.
vcs-rebase-error-from = { $commit } не является более ранним коммитом текущей ветки.
vcs-rebase-error-what = Укажите, что перебазировать: на ветку, начиная с коммита или коммиты для cherry-pick.
vcs-rebase-error-no-commits = В { $branch } нет коммитов для перебазирования на { $onto }.
vcs-rebase-error-too-many = { $count } коммитов — слишком много для одного интерактивного перебазирования (не больше { $limit }): начните с более позднего коммита.
vcs-rebase-error-twice = { $commit } встречается в списке дважды.
vcs-rebase-error-changes = Сначала закоммитьте или спрячьте изменения: перебазирование переписывает файлы.

## Worktrees and submodules (vcs.worktree.*, vcs.submodule.*).
vcs-error-worktree-branch-exists = Локальная ветка { $branch } уже есть: задайте имя новой ветки рабочего дерева.
vcs-error-worktree-folder = Папка { $folder } не пуста: выберите для рабочего дерева другую папку.
vcs-error-no-worktree = { $folder } не является рабочим деревом этого репозитория.
vcs-error-worktree-remove-own = Основное рабочее дерево и рабочее дерево этого проекта удалить нельзя.
vcs-error-worktree-locked = Рабочее дерево { $folder } заблокировано: сначала разблокируйте его.
vcs-error-worktree-lock-main = Основное рабочее дерево заблокировать нельзя.
vcs-error-worktree-gone = Папки рабочего дерева { $folder } больше нет.
vcs-error-no-submodule = { $path } не является подмодулем этого репозитория.
vcs-error-no-submodules = В репозитории нет подмодулей.
vcs-error-submodule-uninitialized = Подмодуль { $path } не инициализирован: сначала инициализируйте его.
vcs-progress-submodule-update = Обновление подмодулей…
vcs-progress-submodule-init = Инициализация подмодулей…
vcs-submodule-updated = { $count ->
    [one] Обновлён { $count } подмодуль
    [few] Обновлено { $count } подмодуля
   *[other] Обновлено { $count } подмодулей
}
# Редактор конфликтов слияния и незавершённая операция (vcs.conflict.*, vcs.operation.*, ADR-0072).
vcs-operation-none = Сейчас не выполняется ни слияние, ни перебазирование, ни cherry-pick, ни отмена коммита, ни применение stash.
vcs-operation-merge = Слияние «{ $theirs }» в «{ $ours }»
vcs-operation-rebase = Перебазирование «{ $branch }» на { $onto }: коммит { $step } из { $total }
vcs-operation-cherry-pick = Cherry-pick { $commit } в «{ $ours }»
vcs-operation-revert = Отмена коммита { $commit } в «{ $ours }»
vcs-operation-stash-apply = Применение { $stash } в «{ $ours }»
vcs-operation-stash-pop = Извлечение { $stash } в «{ $ours }»
vcs-conflict-ours-rebase = на { $onto }
vcs-conflict-error-none = Файлов с конфликтами нет.
vcs-conflict-error-not-conflicted = В файле { $file } нет конфликтов.
vcs-conflict-error-closed = Редактор конфликтов не открыт: укажите файл.
vcs-conflict-error-no-change = В этом файле нет изменения { $change }.
vcs-conflict-error-not-mergeable = Этот файл нельзя слить построчно (он двоичный или одна из сторон его удалила): возьмите нашу или их версию целиком.
vcs-conflict-error-both-file = Обе стороны сразу можно взять только для изменений, а не для всего файла.
vcs-conflict-error-unresolved = { $count ->
    [one] Не разрешён { $count } конфликт
    [few] Не разрешены { $count } конфликта
   *[other] Не разрешено { $count } конфликтов
}: возьмите для него сторону или исправьте результат либо всё равно отметьте файл как разрешённый.

git-workspace-title = Git
git-workspace-graph-tab = Граф
vcs-error-safe-mode = Git выключен: проект открыт в безопасном режиме, где IDE не вызывает Git. Доверьте проекту («Доверять проекту»), и Git запустится без повторного открытия.
